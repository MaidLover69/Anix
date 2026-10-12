import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { PlayerPlaylistDrawer } from '../components/PlayerPlaylistDrawer';
import { ServerSelectDialog } from '../components/ServerSelectDialog';
import { PlayerFeedback } from '../components/player/PlayerFeedback';
import type { FeedbackMessage, PlayerFeedbackHandle } from '../components/player/PlayerFeedback';
import type { EpisodeStep } from '../components/player/PlayerControls';
import { PlayerStatus } from '../components/player/PlayerStatus';
import type { PlayerStatusAction } from '../components/player/PlayerStatus';
import { PlayerTopBar } from '../components/player/PlayerTopBar';
import { VideoPlayer } from '../components/player/VideoPlayer';
import { noPluginsDetail, noPluginsTitle } from '../components/NoPluginsNotice';
import { sameEpisode } from '../components/player/format';
import { errorMessage } from '../components/ui/ErrorState';
import { useAnimeDetails } from '../hooks/useAniList';
import { useFullscreen } from '../hooks/useFullscreen';
import { useAnimeCatalogEpisodes, useEpisodes, useEpisodeSources } from '../hooks/useProviders';
import { useSiteVerification } from '../hooks/useSiteVerification';
import { PLUGIN_SETTINGS_PATH, useStreamingProviders } from '../hooks/usePlugins';
import { AUTO_PROVIDER, DEFAULT_SETTINGS, useSettings } from '../hooks/useSettings';
import { watchPath } from '../hooks/watchProgress';
import { useStreamProxyBase } from '../lib/streamProxy';
import { usePlayerStore } from '../stores/playerStore';
import type { CustomPlaylistItem, EpisodeItem } from '../types';

type Category = 'sub' | 'dub';

interface EpisodeTarget {
  anilistId: number;
  episode: number;
  /** Null keeps the current audio version. */
  category: Category | null;
  label: string;
  /** True when it comes from the episodes queued in the drawer. */
  queued?: boolean;
}

const NO_EPISODES: number[] = [];

function parseAnimeId(value: string | undefined): number | null {
  if (!value || !/^\d+$/.test(value)) return null;
  const id = Number(value);
  return Number.isSafeInteger(id) && id > 0 ? id : null;
}

function parseEpisodeNumber(value: string | undefined): number | null {
  if (!value || !/^\d+(\.\d+)?$/.test(value)) return null;
  const episode = Number(value);
  return Number.isFinite(episode) ? episode : null;
}

/** Position of the current episode in the playlist queue, or -1 when it is not part of it. */
function findQueuePosition(
  queue: CustomPlaylistItem[],
  preferredIndex: number,
  anilistId: number,
  episodeNumber: number
): number {
  const matches = (item: CustomPlaylistItem | undefined) =>
    !!item && item.anilist_id === anilistId && sameEpisode(item.episode_num, episodeNumber);
  if (matches(queue[preferredIndex])) return preferredIndex;
  for (let i = preferredIndex + 1; i < queue.length; i++) if (matches(queue[i])) return i;
  return queue.findIndex(matches);
}

function queueTarget(item: CustomPlaylistItem): EpisodeTarget {
  return {
    anilistId: item.anilist_id,
    episode: item.episode_num,
    category: item.category === 'dub' ? 'dub' : item.category === 'sub' ? 'sub' : null,
    label: `${item.series_title} · Episode ${item.episode_num}`,
  };
}

export const Watch: React.FC = () => {
  const { id, episodeNum } = useParams<{ id: string; episodeNum: string }>();
  const navigate = useNavigate();
  const anilistId = parseAnimeId(id);
  const episodeNumber = parseEpisodeNumber(episodeNum);

  if (anilistId === null || episodeNumber === null) {
    return (
      <div className="dark-scope relative h-full w-full overflow-hidden bg-black">
        <PlayerStatus
          kind="error"
          title="Episode not found"
          detail="This link does not point to an anime episode."
          actions={[{ label: 'Go to Home', onClick: () => navigate('/', { replace: true }), primary: true }]}
        />
      </div>
    );
  }

  return <WatchSession anilistId={anilistId} episodeNumber={episodeNumber} />;
};

interface WatchSessionProps {
  anilistId: number;
  episodeNumber: number;
}

/**
 * Everything around the video: what to play (from the URL, with settings as defaults),
 * resolving it, moving between episodes, and the dialogs. The element rendered here is the
 * one shown fullscreen, so it outlives the per-episode `VideoPlayer` inside it.
 */
const WatchSession: React.FC<WatchSessionProps> = ({ anilistId, episodeNumber }) => {
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();

  const [container, setContainer] = useState<HTMLDivElement | null>(null);
  const { isFullscreen, toggle: toggleFullscreen } = useFullscreen(container);

  const feedbackRef = useRef<PlayerFeedbackHandle>(null);
  const notify = useCallback((message: FeedbackMessage) => feedbackRef.current?.show(message), []);

  const [serversOpen, setServersOpen] = useState(false);
  const [episodesOpen, setEpisodesOpen] = useState(false);

  const settingsQuery = useSettings();
  const settings = settingsQuery.data ?? DEFAULT_SETTINGS;
  const settingsReady = !settingsQuery.isPending;

  // Provider keys are plugin ids; which ones exist, and their names, come from the plugin list.
  const providers = useStreamingProviders();
  const providerLabel = providers.label;
  const noPlugins = providers.none;

  const detailsQuery = useAnimeDetails(anilistId);
  const details = detailsQuery.data ?? null;
  const malId: number | null = details?.idMal ?? null;
  const titleRomaji: string | null = details?.title?.romaji ?? null;
  const animeCover: string | null =
    details?.coverImage?.extraLarge || details?.coverImage?.large || null;

  // The all-providers listing is only fetched for the server dialog. Whatever is already
  // cached (the detail page loads it) is used right away.
  const episodesQuery = useEpisodes(anilistId, malId, titleRomaji, {
    enabled: !!details && serversOpen,
  });
  const episodesData = episodesQuery.data;

  // Category and provider come from the URL on every render; settings fill in what the URL
  // leaves open. `prov=auto`, a missing `prov` and an unknown key all mean "no choice".
  const urlCategory = searchParams.get('cat');
  const category: Category =
    urlCategory === 'sub' || urlCategory === 'dub' ? urlCategory : settings.prefer_dub ? 'dub' : 'sub';

  // Provider keys picked in the server dialog during this visit, which may be newer than
  // the plugin list this render has.
  const pickedProviders = useRef(new Set<string>());
  const knownProvider = (key: string | null | undefined): string | null =>
    key && key !== AUTO_PROVIDER && (providers.accepts(key) || pickedProviders.current.has(key))
      ? key
      : null;
  const explicitProvider = knownProvider(searchParams.get('prov'));
  const preferredProvider = knownProvider(settings.preferred_provider);
  const requestedProvider = explicitProvider ?? preferredProvider;

  const sourcesQuery = useEpisodeSources({
    anilistId,
    malId,
    episodeNumber,
    category,
    titleRomaji,
    provider: requestedProvider,
    // A provider from settings is a preference: if it has nothing, every provider is asked.
    fallbackToAny: explicitProvider === null && preferredProvider !== null,
    // Which provider to ask is not known before the plugin list is, and with no usable
    // plugin there is nobody to ask.
    enabled: !!details && settingsReady && providers.settled && !noPlugins,
  });
  const sources = sourcesQuery.data;
  const servingProvider = sources?.provider || null;

  const proxyQuery = useStreamProxyBase();

  const catalogQuery = useAnimeCatalogEpisodes(
    anilistId,
    malId,
    details?.episodes,
    titleRomaji,
    details?.status,
    details?.nextAiringEpisode?.episode,
    { enabled: !!details }
  );
  const catalogEpisodes = catalogQuery.data;

  // The serving provider's own list when it is known, otherwise the catalogue, otherwise
  // the released episode count from AniList.
  const episodes = useMemo<EpisodeItem[]>(() => {
    const listProvider = servingProvider ?? explicitProvider;
    const providerList = listProvider ? episodesData?.[listProvider]?.[category] ?? [] : [];
    let list: EpisodeItem[] = providerList;
    if (list.length === 0) list = catalogEpisodes ?? [];
    if (list.length === 0) {
      const nextAiring: number | null = details?.nextAiringEpisode?.episode ?? null;
      const released = nextAiring && nextAiring > 0 ? nextAiring - 1 : details?.episodes ?? 0;
      list = Array.from({ length: Math.max(0, released) }, (_, index) => ({
        number: index + 1,
        pipe_id: `${anilistId}-${index + 1}`,
        title: null,
        image: null,
        filler: false,
      }));
    }
    return [...list].sort((a, b) => a.number - b.number);
  }, [servingProvider, explicitProvider, episodesData, category, catalogEpisodes, details, anilistId]);

  const currentEpisode = useMemo(
    () => episodes.find((ep) => sameEpisode(ep.number, episodeNumber)) ?? null,
    [episodes, episodeNumber]
  );
  const episodeTitle = currentEpisode?.title?.trim() || null;

  // Playlist queue from the Library. It only applies while the episode on screen is part of it.
  const customQueue = usePlayerStore((s) => s.customQueue);
  const customQueueIndex = usePlayerStore((s) => s.customQueueIndex);
  const activePlaylistName = usePlayerStore((s) => s.activePlaylistName);
  const queuePosition = useMemo(
    () => findQueuePosition(customQueue, customQueueIndex, anilistId, episodeNumber),
    [customQueue, customQueueIndex, anilistId, episodeNumber]
  );
  const inPlaylist = queuePosition !== -1;

  useEffect(() => {
    if (customQueue.length === 0) return;
    const store = usePlayerStore.getState();
    if (queuePosition === -1) store.clearCustomQueue();
    else if (queuePosition !== customQueueIndex) store.jumpToCustomQueueIndex(queuePosition);
  }, [customQueue, customQueueIndex, queuePosition]);

  // Leaving the player ends the playlist. The hash check keeps React's development-only
  // remount from doing the same.
  useEffect(
    () => () => {
      if (!window.location.hash.startsWith('#/watch/')) usePlayerStore.getState().clearCustomQueue();
    },
    []
  );

  // Episodes queued in the drawer, for this anime only.
  const [drawerQueue, setDrawerQueue] = useState<{ animeId: number; episodes: number[] }>({
    animeId: anilistId,
    episodes: [],
  });
  const queuedEpisodes = drawerQueue.animeId === anilistId ? drawerQueue.episodes : NO_EPISODES;
  const pendingQueue = useMemo(
    () => queuedEpisodes.filter((n) => !sameEpisode(n, episodeNumber)),
    [queuedEpisodes, episodeNumber]
  );

  const animeTitle: string =
    details?.title?.english ||
    details?.title?.userPreferred ||
    details?.title?.romaji ||
    (inPlaylist ? customQueue[queuePosition].series_title : '');

  const previousTarget = useMemo<EpisodeTarget | null>(() => {
    if (inPlaylist) return queuePosition > 0 ? queueTarget(customQueue[queuePosition - 1]) : null;
    const before = episodes.filter((ep) => ep.number < episodeNumber - 0.001);
    const ep = before[before.length - 1];
    return ep ? { anilistId, episode: ep.number, category: null, label: `Episode ${ep.number}` } : null;
  }, [inPlaylist, queuePosition, customQueue, episodes, episodeNumber, anilistId]);

  const nextTarget = useMemo<EpisodeTarget | null>(() => {
    if (inPlaylist) {
      return queuePosition < customQueue.length - 1 ? queueTarget(customQueue[queuePosition + 1]) : null;
    }
    if (pendingQueue.length > 0) {
      const episode = pendingQueue[0];
      return { anilistId, episode, category: null, label: `Queued: Episode ${episode}`, queued: true };
    }
    const ep = episodes.find((candidate) => candidate.number > episodeNumber + 0.001);
    return ep ? { anilistId, episode: ep.number, category: null, label: `Episode ${ep.number}` } : null;
  }, [inPlaylist, queuePosition, customQueue, pendingQueue, episodes, episodeNumber, anilistId]);

  // Set when an episode ended and the next one was started for the user: that one plays
  // even if "autoplay on open" is off.
  const continuingRef = useRef(false);

  // Moving around inside the player replaces the history entry, so Back leaves the player.
  const goTo = useCallback(
    (target: EpisodeTarget, continuing: boolean = false) => {
      continuingRef.current = continuing;
      if (target.queued) {
        setDrawerQueue((prev) => ({
          animeId: target.anilistId,
          episodes: (prev.animeId === target.anilistId ? prev.episodes : []).filter(
            (n) => !sameEpisode(n, target.episode)
          ),
        }));
      }
      const provider = target.anilistId === anilistId ? explicitProvider : null;
      navigate(watchPath(target.anilistId, target.episode, target.category ?? category, provider), {
        replace: true,
      });
    },
    [anilistId, category, explicitProvider, navigate]
  );

  const handleBack = useCallback(() => {
    const historyIndex: unknown = window.history.state?.idx;
    if (typeof historyIndex === 'number' && historyIndex > 0) navigate(-1);
    else navigate(`/detail/${anilistId}`, { replace: true });
  }, [anilistId, navigate]);

  const setRouteChoice = useCallback(
    (nextCategory: Category, provider: string) => {
      setSearchParams(
        (prev) => {
          const params = new URLSearchParams(prev);
          params.set('cat', nextCategory);
          params.set('prov', provider);
          return params;
        },
        { replace: true }
      );
    },
    [setSearchParams]
  );

  const handleToggleCategory = useCallback(
    (nextCategory: Category) => {
      if (nextCategory !== category) setRouteChoice(nextCategory, explicitProvider ?? AUTO_PROVIDER);
    },
    [category, explicitProvider, setRouteChoice]
  );

  const handleSelectServer = useCallback(
    (provider: string, nextCategory: Category) => {
      pickedProviders.current.add(provider);
      setRouteChoice(nextCategory, provider);
    },
    [setRouteChoice]
  );

  const { refetch: refetchSources } = sourcesQuery;
  const handleReload = useCallback(() => {
    refetchSources();
  }, [refetchSources]);
  // A site that wants the user to pass its check is offered for verification next to Retry;
  // the lookup is repeated by itself once the site answers.
  const verification = useSiteVerification({
    enabled: sourcesQuery.isError && !sourcesQuery.isFetching,
    attempt: sourcesQuery.errorUpdatedAt,
    provider: explicitProvider,
    onVerified: handleReload,
  });
  const handleTryOtherServers = explicitProvider
    ? () => setRouteChoice(category, AUTO_PROVIDER)
    : undefined;
  const openServers = useCallback(() => setServersOpen(true), []);
  const openEpisodes = useCallback(() => setEpisodesOpen(true), []);

  const autoPlayNext = usePlayerStore((s) => s.autoPlayNext);
  const handleEnded = () => {
    // Queued episodes always continue; otherwise the auto-play-next preference decides.
    // At the last episode there is no target and nothing happens.
    if (nextTarget && (nextTarget.queued || autoPlayNext)) goTo(nextTarget, true);
  };

  const previousStep: EpisodeStep | null = previousTarget && {
    label: `Previous: ${previousTarget.label} (Shift+P)`,
    onClick: () => goTo(previousTarget),
  };
  const nextStep: EpisodeStep | null = nextTarget && {
    label: `Next: ${nextTarget.label} (Shift+N)`,
    onClick: () => goTo(nextTarget),
  };

  const shownProvider = servingProvider ?? (sourcesQuery.isFetching ? explicitProvider : null);
  const metaLine = [
    `Episode ${episodeNumber}`,
    category === 'dub' ? 'Dub' : 'Sub',
    shownProvider ? providerLabel(shownProvider) : null,
    inPlaylist
      ? `${activePlaylistName ?? 'Playlist'} ${queuePosition + 1}/${customQueue.length}`
      : null,
  ]
    .filter(Boolean)
    .join(' · ');

  const sourceErrorActions: PlayerStatusAction[] = [
    ...verification.sites.map((site) => ({
      label: `Verify ${site.label}`,
      onClick: () => verification.verify(site.site),
      primary: true,
    })),
    { label: 'Retry', onClick: handleReload, primary: verification.sites.length === 0 },
    { label: 'Change server', onClick: openServers },
    ...(handleTryOtherServers ? [{ label: 'Try other servers', onClick: handleTryOtherServers }] : []),
  ];

  let status: React.ReactNode = null;
  let playable = false;
  if (!details && detailsQuery.isError) {
    status = (
      <PlayerStatus
        kind="error"
        title="This anime could not be loaded"
        detail={errorMessage(detailsQuery.error)}
        actions={[{ label: 'Retry', onClick: () => detailsQuery.refetch(), primary: true }]}
      />
    );
  } else if (!details || !settingsReady || !providers.settled) {
    status = <PlayerStatus kind="loading" title="Loading…" />;
  } else if (noPlugins && !sources) {
    // A stream that is already playing is left alone if the plugins go away under it.
    status = (
      <PlayerStatus
        kind="setup"
        title={noPluginsTitle(providers.installed)}
        detail={`${noPluginsDetail(providers.installed)} Episode ${episodeNumber} cannot be played until then.`}
        actions={[
          { label: 'Open plugin settings', onClick: () => navigate(PLUGIN_SETTINGS_PATH), primary: true },
          { label: 'Back', onClick: handleBack },
        ]}
      />
    );
  } else if (sourcesQuery.isFetching || sourcesQuery.isPending) {
    status = (
      <PlayerStatus
        kind="loading"
        title={`Finding a stream for Episode ${episodeNumber}…`}
        detail={
          sourcesQuery.fetchStatus === 'paused'
            ? 'Waiting for a network connection.'
            : requestedProvider
            ? `Asking ${providerLabel(requestedProvider)}. This can take up to half a minute.`
            : 'Asking the providers. This can take up to half a minute.'
        }
      />
    );
  } else if (sourcesQuery.isError) {
    status = (
      <PlayerStatus
        kind="error"
        title={`No stream found for Episode ${episodeNumber}`}
        detail={errorMessage(sourcesQuery.error, 'The providers did not return a stream.')}
        actions={sourceErrorActions}
      />
    );
  } else if (!sources || sources.streams.length === 0) {
    status = (
      <PlayerStatus
        kind="error"
        title={`No stream found for Episode ${episodeNumber}`}
        detail="The provider answered, but without any stream for this episode."
        actions={sourceErrorActions}
      />
    );
  } else if (proxyQuery.isPending) {
    status = <PlayerStatus kind="loading" title="Preparing playback…" />;
  } else if (!proxyQuery.data) {
    status = (
      <PlayerStatus
        kind="error"
        title="The stream proxy is not available"
        detail={errorMessage(proxyQuery.error, 'The backend did not report a proxy address.')}
        actions={[{ label: 'Retry', onClick: () => proxyQuery.refetch(), primary: true }]}
      />
    );
  } else {
    playable = true;
  }

  return (
    <div ref={setContainer} className="dark-scope relative h-full w-full overflow-hidden bg-black">
      {playable && sources && proxyQuery.data ? (
        <VideoPlayer
          key={`${anilistId}:${episodeNumber}:${category}:${requestedProvider ?? AUTO_PROVIDER}:${sourcesQuery.dataUpdatedAt}`}
          anilistId={anilistId}
          malId={malId}
          animeTitle={animeTitle || `Anime ${anilistId}`}
          animeCover={animeCover}
          episodeNumber={episodeNumber}
          episodeTitle={episodeTitle}
          category={category}
          metaLine={metaLine}
          sources={sources}
          provider={servingProvider ?? requestedProvider ?? AUTO_PROVIDER}
          providerName={servingProvider ? providerLabel(servingProvider) : null}
          proxyBase={proxyQuery.data}
          loadKey={sourcesQuery.dataUpdatedAt}
          autoplay={settings.autoplay || continuingRef.current}
          autoSkip={settings.auto_skip_intro_outro}
          externalPlayer={settings.external_player === 'vlc' ? 'vlc' : 'mpv'}
          previous={previousStep}
          next={nextStep}
          queuedCount={pendingQueue.length}
          onEnded={handleEnded}
          onBack={handleBack}
          onOpenEpisodes={openEpisodes}
          onOpenServers={openServers}
          onReload={handleReload}
          onTryOtherServers={handleTryOtherServers}
          onToggleCategory={handleToggleCategory}
          isFullscreen={isFullscreen}
          onToggleFullscreen={toggleFullscreen}
          overlayOpen={serversOpen || episodesOpen}
          container={container}
          notify={notify}
        />
      ) : (
        <>
          <PlayerTopBar
            title={animeTitle || (detailsQuery.isError ? 'Anime' : 'Loading…')}
            meta={metaLine}
            onBack={handleBack}
          />
          {status}
        </>
      )}

      <PlayerFeedback ref={feedbackRef} />

      <PlayerPlaylistDrawer
        isOpen={episodesOpen}
        onClose={() => setEpisodesOpen(false)}
        episodes={episodes}
        currentEpisodeNumber={episodeNumber}
        onSelectEpisode={(episode) =>
          goTo({ anilistId, episode, category: null, label: `Episode ${episode}` })
        }
        queuedEpisodes={queuedEpisodes}
        onToggleQueueEpisode={(episode) =>
          setDrawerQueue((prev) => {
            const current = prev.animeId === anilistId ? prev.episodes : [];
            return {
              animeId: anilistId,
              episodes: current.some((n) => sameEpisode(n, episode))
                ? current.filter((n) => !sameEpisode(n, episode))
                : [...current, episode],
            };
          })
        }
        onPlayQueue={() => {
          if (pendingQueue.length > 0) {
            goTo({
              anilistId,
              episode: pendingQueue[0],
              category: null,
              label: `Episode ${pendingQueue[0]}`,
              queued: true,
            });
          }
        }}
        onClearQueue={() => setDrawerQueue({ animeId: anilistId, episodes: [] })}
        container={container}
      />

      <ServerSelectDialog
        isOpen={serversOpen}
        onClose={() => setServersOpen(false)}
        anilistId={anilistId}
        animeTitle={animeTitle}
        episode={
          currentEpisode ?? {
            number: episodeNumber,
            pipe_id: `${anilistId}-${episodeNumber}`,
            title: null,
            image: null,
            filler: false,
          }
        }
        episodesData={episodesData}
        isLoading={episodesQuery.isFetching && !episodesData}
        errorMessage={episodesQuery.isError && !episodesData ? errorMessage(episodesQuery.error) : null}
        initialCategory={category}
        activeProvider={servingProvider}
        onSelectServer={handleSelectServer}
        container={container}
      />
    </div>
  );
};
