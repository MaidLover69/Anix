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
import { useEmbeddedMpvStatus } from '../hooks/useEmbeddedMpv';
import { noPluginsDetail, noPluginsTitle } from '../components/NoPluginsNotice';
import { sameEpisode } from '../components/player/format';
import { errorMessage } from '../components/ui/ErrorState';
import { useAnimeDetails } from '../hooks/useAniList';
import { useFullscreen } from '../hooks/useFullscreen';
import { useMiniPlayerSession } from '../hooks/useMiniPlayer';
import { useAnimeCatalogEpisodes, useEpisodes, useEpisodeSources } from '../hooks/useProviders';
import { useSiteVerification } from '../hooks/useSiteVerification';
import { PLUGIN_SETTINGS_PATH, useStreamingProviders } from '../hooks/usePlugins';
import { AUTO_PROVIDER, DEFAULT_SETTINGS, useSettings } from '../hooks/useSettings';
import { watchPath } from '../hooks/watchProgress';
import { useStreamProxyBase } from '../lib/streamProxy';
import { describeStream, sourceLabel } from '../lib/languages';
import { splitPlayFailure } from '../lib/playFailure';
import { episodeExists, parseAnimeId } from '../lib/routes';
import { useRememberedSource } from '../hooks/useRememberedSource';
import { invoke } from '@tauri-apps/api/core';
import {
  LOCAL_PROVIDER,
  localProviderLabel,
  useLocalEpisodeSource,
  useLocalMedia,
} from '../hooks/useLocalMedia';
import { usePlayerStore } from '../stores/playerStore';
import { t } from '../i18n';
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
/** The "embedded mpv did not start" notice is shown once per run of the app. */
let mpvStartNoticeShown = false;

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
    label: t('watch.seriesEpisode', { title: item.series_title, number: item.episode_num }),
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
          title={t('watch.episodeNotFound')}
          detail={t('watch.episodeNotFoundDetail')}
          actions={[{ label: t('watch.goHome'), onClick: () => navigate('/', { replace: true }), primary: true }]}
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
  // The mini-player shows only the video, over the rest of the app's window.
  const miniPlayer = useMiniPlayerSession();

  const feedbackRef = useRef<PlayerFeedbackHandle>(null);
  const notify = useCallback((message: FeedbackMessage) => feedbackRef.current?.show(message), []);

  const [serversOpen, setServersOpen] = useState(false);
  const [episodesOpen, setEpisodesOpen] = useState(false);

  const settingsQuery = useSettings();
  const settings = settingsQuery.data ?? DEFAULT_SETTINGS;
  const settingsReady = !settingsQuery.isPending;
  // Embedded mpv when this run started it; a stream it cannot play falls back to the web player.
  // A change of the engine setting applies after a restart, so the running engine decides.
  const mpvStatus = useEmbeddedMpvStatus();
  const [mpvFailed, setMpvFailed] = useState(false);
  const useMpv = mpvStatus.data?.available === true && !mpvFailed;
  const handleMpvFail = useCallback(
    (message: string) => {
      setMpvFailed(true);
      notify({ text: t('watch.switchedToWebPlayer', { message }), tone: 'error' });
    },
    [notify]
  );
  // Embedded mpv was wanted but did not start (libmpv missing, GL failed): say why, once a run.
  const mpvStartError = mpvStatus.data?.requested && !mpvStatus.data.available ? mpvStatus.data.error : null;
  useEffect(() => {
    if (!mpvStartError || mpvStartNoticeShown) return;
    mpvStartNoticeShown = true;
    notify({ text: t('watch.usingWebPlayer', { message: mpvStartError }), tone: 'error' });
  }, [mpvStartError, notify]);

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

  // Category and provider come from the URL on every render; settings fill in what the URL
  // leaves open. `prov=auto`, a missing `prov` and an unknown key all mean "no choice".
  const urlCategory = searchParams.get('cat');
  // The server last picked for this anime goes first, with its audio, until the URL says otherwise.
  const { remembered } = useRememberedSource(anilistId);
  const rememberedUsable = !!remembered && providers.accepts(remembered.provider);
  const category: Category =
    urlCategory === 'sub' || urlCategory === 'dub'
      ? urlCategory
      : rememberedUsable
      ? remembered.category
      : settings.prefer_dub
      ? 'dub'
      : 'sub';

  // Provider keys picked in the server dialog during this visit, which may be newer than
  // the plugin list this render has.
  const pickedProviders = useRef(new Set<string>());
  const knownProvider = (key: string | null | undefined): string | null =>
    key && key !== AUTO_PROVIDER && (providers.accepts(key) || pickedProviders.current.has(key))
      ? key
      : null;
  const explicitProvider = knownProvider(searchParams.get('prov'));
  const preferredProvider =
    (remembered?.category === category ? knownProvider(remembered.provider) : null) ??
    knownProvider(settings.preferred_provider);
  const requestedProvider = explicitProvider ?? preferredProvider;
  // "Your files": the episode plays from a file linked to the title, through the proxy.
  const localRequested = searchParams.get('prov') === LOCAL_PROVIDER;
  const localQuery = useLocalEpisodeSource(anilistId, episodeNumber, localRequested);
  const localEpisodes = useLocalMedia(anilistId).episodes;
  // What the URL keeps when only the audio or the episode changes.
  const routeProvider = localRequested ? LOCAL_PROVIDER : explicitProvider;

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

  // /watch/<id>/0 or /9999: say the episode does not exist instead of asking every extension.
  // A number any list has counts (some shows have an episode 0 or 12.5), so a doubtful one
  // waits for the catalogue's list before it is refused.
  const knownEpisodes = useMemo(() => {
    if (!details || localRequested) return null;
    const nextAiring: number | null = details.nextAiringEpisode?.episode ?? null;
    return {
      total: details.episodes ?? null,
      aired: nextAiring && nextAiring > 0 ? nextAiring - 1 : null,
      airing: details.status === 'RELEASING' || !!nextAiring,
      listed: [...(catalogEpisodes ?? []).map((ep) => ep.number), ...localEpisodes],
    };
  }, [details, localRequested, catalogEpisodes, localEpisodes]);
  const notInCatalogue = knownEpisodes !== null && !episodeExists(episodeNumber, knownEpisodes);
  // A whole number above the catalogue's count may be an extra episode an extension lists
  // (episode 13 of a 12-episode entry), so the extensions' lists are read before refusing it.
  const checkExtensionLists =
    notInCatalogue && !catalogQuery.isPending && Number.isInteger(episodeNumber) && episodeNumber >= 1;

  // The all-providers listing is otherwise only fetched for the server dialog. Whatever is
  // already cached (the detail page loads it) is used right away.
  const episodesQuery = useEpisodes(anilistId, malId, titleRomaji, {
    enabled: !!details && (serversOpen || checkExtensionLists),
  });
  const episodesData = episodesQuery.data;

  const episodeMissing = useMemo(() => {
    if (!notInCatalogue || !knownEpisodes) return false;
    const fromExtensions = Object.values(episodesData ?? {}).flatMap((p) =>
      [...(p?.sub ?? []), ...(p?.dub ?? [])].map((ep) => ep.number)
    );
    return !episodeExists(episodeNumber, { ...knownEpisodes, listed: [...knownEpisodes.listed, ...fromExtensions] });
  }, [notInCatalogue, knownEpisodes, episodesData, episodeNumber]);
  const episodeCheckPending = catalogQuery.isPending || (checkExtensionLists && episodesQuery.isPending);

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
    enabled: !!details && settingsReady && providers.settled && !noPlugins && !localRequested && !episodeMissing,
  });
  const sources = localRequested ? localQuery.data?.sources : sourcesQuery.data;
  const servingProvider = sources?.provider || null;
  // What each server's stream was like when it played in this visit, for the server list.
  const seenStreams = useRef<Record<string, string>>({});
  const servingDescription = servingProvider && !localRequested ? describeStream(sources, category) : null;
  if (servingProvider && servingDescription) seenStreams.current[`${servingProvider}:${category}`] = servingDescription;

  const proxyQuery = useStreamProxyBase();


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
    if (localRequested) {
      const missing = localEpisodes
        .filter((n) => !list.some((ep) => sameEpisode(ep.number, n)))
        .map((n) => ({ number: n, pipe_id: `local-${n}`, title: null, image: null, filler: false }));
      list = [...list, ...missing];
    }
    return [...list].sort((a, b) => a.number - b.number);
  }, [
    servingProvider,
    explicitProvider,
    episodesData,
    category,
    catalogEpisodes,
    details,
    anilistId,
    localRequested,
    localEpisodes,
  ]);

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
    const before = episodes.filter((ep) => ep.number < episodeNumber - 0.001); // i18n-ignore
    const ep = before[before.length - 1];
    return ep ? { anilistId, episode: ep.number, category: null, label: t('common.episodeNumber', { number: ep.number }) } : null;
  }, [inPlaylist, queuePosition, customQueue, episodes, episodeNumber, anilistId]);

  const nextTarget = useMemo<EpisodeTarget | null>(() => {
    if (inPlaylist) {
      return queuePosition < customQueue.length - 1 ? queueTarget(customQueue[queuePosition + 1]) : null;
    }
    if (pendingQueue.length > 0) {
      const episode = pendingQueue[0];
      return { anilistId, episode, category: null, label: t('watch.queuedEpisode', { number: episode }), queued: true };
    }
    const ep = episodes.find((candidate) => candidate.number > episodeNumber + 0.001);
    return ep ? { anilistId, episode: ep.number, category: null, label: t('common.episodeNumber', { number: ep.number }) } : null;
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
      const provider = target.anilistId === anilistId ? routeProvider : null;
      navigate(watchPath(target.anilistId, target.episode, target.category ?? category, provider), {
        replace: true,
      });
    },
    [anilistId, category, routeProvider, navigate]
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
      if (nextCategory !== category) setRouteChoice(nextCategory, routeProvider ?? AUTO_PROVIDER);
    },
    [category, routeProvider, setRouteChoice]
  );

  const handleSelectServer = useCallback(
    (provider: string, nextCategory: Category) => {
      pickedProviders.current.add(provider);
      setRouteChoice(nextCategory, provider);
    },
    [setRouteChoice]
  );

  const { refetch: refetchSources } = sourcesQuery;
  const { refetch: refetchLocal } = localQuery;
  const handleReload = useCallback(() => {
    if (localRequested) refetchLocal();
    else refetchSources();
  }, [localRequested, refetchLocal, refetchSources]);
  // A site that wants the user to pass its check is offered for verification next to Retry;
  // the lookup is repeated by itself once the site answers.
  const verification = useSiteVerification({
    enabled: sourcesQuery.isError && !sourcesQuery.isFetching,
    attempt: sourcesQuery.errorUpdatedAt,
    provider: explicitProvider,
    onVerified: handleReload,
  });
  const handleTryOtherServers =
    explicitProvider && !localRequested ? () => setRouteChoice(category, AUTO_PROVIDER) : undefined;
  const streamInstead = !noPlugins ? () => setRouteChoice(category, AUTO_PROVIDER) : undefined;
  const localFile = localQuery.data?.file_path ?? null;
  const externalPlayerName = settings.external_player === 'vlc' ? 'VLC' : 'mpv';
  const openLocalExternally = useCallback(() => {
    if (!localFile) return;
    invoke('open_file_in_player', {
      filePath: localFile,
      player: settings.external_player === 'vlc' ? 'vlc' : 'mpv',
    })
      .then(() => notify({ text: t('watch.openedIn', { player: externalPlayerName }) }))
      .catch((err) =>
        notify({ text: t('watch.couldNotOpenExternal', { message: errorMessage(err) }), tone: 'error' })
      );
  }, [localFile, settings.external_player, externalPlayerName, notify]);
  const localErrorActions: PlayerStatusAction[] = localRequested
    ? [
        ...(localFile ? [{ label: t('watch.openIn', { player: externalPlayerName }), onClick: openLocalExternally }] : []),
        ...(streamInstead ? [{ label: t('watch.streamInstead'), onClick: streamInstead }] : []),
      ]
    : [];
  const openServers = useCallback(() => setServersOpen(true), []);
  const openEpisodes = useCallback(() => setEpisodesOpen(true), []);

  const autoPlayNext = usePlayerStore((s) => s.autoPlayNext);
  const handleEnded = () => {
    // Queued episodes always continue; otherwise the auto-play-next preference decides.
    // At the last episode there is no target and nothing happens.
    if (nextTarget && (nextTarget.queued || autoPlayNext)) goTo(nextTarget, true);
  };

  const previousStep: EpisodeStep | null = previousTarget && {
    label: t('watch.previousStep', { label: previousTarget.label }),
    onClick: () => goTo(previousTarget),
  };
  const nextStep: EpisodeStep | null = nextTarget && {
    label: t('watch.nextStep', { label: nextTarget.label }),
    onClick: () => goTo(nextTarget),
  };

  const shownProvider = servingProvider ?? (sourcesQuery.isFetching ? explicitProvider : null);
  const metaLine = [
    t('common.episodeNumber', { number: episodeNumber }),
    localRequested
      ? null
      : sources?.language
      ? sourceLabel(sources.language, category)
      : category === 'dub'
      ? t('common.dub')
      : t('common.sub'),
    localRequested ? localProviderLabel() : shownProvider ? providerLabel(shownProvider) : null,
    inPlaylist
      ? t('watch.playlistPosition', {
          name: activePlaylistName ?? t('watch.playlist'),
          position: queuePosition + 1,
          total: customQueue.length,
        })
      : null,
  ]
    .filter(Boolean)
    .join(' · ');

  const sourceErrorActions: PlayerStatusAction[] = [
    ...verification.sites.map((site) => ({
      label: t('watch.verifySite', { site: site.label }),
      onClick: () => verification.verify(site.site),
      primary: true,
    })),
    { label: t('common.retry'), onClick: handleReload, primary: verification.sites.length === 0 },
    { label: t('watch.changeServer'), onClick: openServers },
    ...(handleTryOtherServers ? [{ label: t('watch.tryOtherServers'), onClick: handleTryOtherServers }] : []),
  ];

  let status: React.ReactNode = null;
  let playable = false;
  if (!details && detailsQuery.isError) {
    status = (
      <PlayerStatus
        kind="error"
        title={t('watch.animeLoadFailed')}
        detail={errorMessage(detailsQuery.error)}
        actions={[{ label: t('common.retry'), onClick: () => detailsQuery.refetch(), primary: true }]}
      />
    );
  } else if (!details || !settingsReady || !providers.settled || (episodeMissing && episodeCheckPending)) {
    status = <PlayerStatus kind="loading" title={t('common.loading')} />;
  } else if (episodeMissing) {
    status = (
      <PlayerStatus
        kind="error"
        title={t('watch.episodeNotFound')}
        detail={t('watch.episodeNotFoundDetail')}
        actions={[{ label: t('router.openAnime'), onClick: () => navigate(`/detail/${anilistId}`, { replace: true }), primary: true }]}
      />
    );
  } else if (localRequested && localQuery.isPending) {
    status = <PlayerStatus kind="loading" title={t('watch.openingLocalFile', { number: episodeNumber })} />;
  } else if (localRequested && localQuery.isError) {
    status = (
      <PlayerStatus
        kind="error"
        title={t('watch.localFileFailed', { number: episodeNumber })}
        detail={errorMessage(localQuery.error)}
        actions={[
          { label: t('common.retry'), onClick: handleReload, primary: true },
          ...(streamInstead ? [{ label: t('watch.streamInstead'), onClick: streamInstead }] : []),
          { label: t('common.back'), onClick: handleBack },
        ]}
      />
    );
  } else if (localRequested && !localQuery.data) {
    status = (
      <PlayerStatus
        kind="error"
        title={t('watch.notInFiles', { number: episodeNumber })}
        detail={t('watch.notInFilesDetail')}
        actions={[
          ...(streamInstead ? [{ label: t('watch.streamInstead'), onClick: streamInstead, primary: true }] : []),
          { label: t('common.back'), onClick: handleBack, primary: !streamInstead },
        ]}
      />
    );
  } else if (noPlugins && !sources) {
    // A stream that is already playing is left alone if the plugins go away under it.
    status = (
      <PlayerStatus
        kind="setup"
        title={noPluginsTitle(providers.installed)}
        detail={t('watch.noPluginsEpisode', { detail: noPluginsDetail(providers.installed), number: episodeNumber })}
        actions={[
          { label: t('watch.openPluginSettings'), onClick: () => navigate(PLUGIN_SETTINGS_PATH), primary: true },
          { label: t('common.back'), onClick: handleBack },
        ]}
      />
    );
  } else if (!localRequested && (sourcesQuery.isFetching || sourcesQuery.isPending)) {
    status = (
      <PlayerStatus
        kind="loading"
        title={t('watch.findingStream', { number: episodeNumber })}
        detail={
          sourcesQuery.fetchStatus === 'paused'
            ? t('watch.waitingForNetwork')
            : requestedProvider
            ? t('watch.askingProvider', { provider: providerLabel(requestedProvider) })
            : t('watch.askingProviders')
        }
      />
    );
  } else if (!localRequested && sourcesQuery.isError) {
    // The reason in plain words; the list per server only under Details.
    const failure = splitPlayFailure(errorMessage(sourcesQuery.error, t('watch.noStreamFallback')));
    status = (
      <PlayerStatus
        kind="error"
        title={t('watch.noStreamFound', { number: episodeNumber })}
        detail={failure.reason}
        moreDetail={failure.details}
        actions={sourceErrorActions}
      />
    );
  } else if (!sources || sources.streams.length === 0) {
    status = (
      <PlayerStatus
        kind="error"
        title={t('watch.noStreamFound', { number: episodeNumber })}
        detail={t('watch.noStreamInAnswer')}
        actions={sourceErrorActions}
      />
    );
  } else if (proxyQuery.isPending) {
    status = <PlayerStatus kind="loading" title={t('watch.preparingPlayback')} />;
  } else if (!proxyQuery.data) {
    status = (
      <PlayerStatus
        kind="error"
        title={t('watch.proxyUnavailable')}
        detail={errorMessage(proxyQuery.error, t('watch.proxyNoAddress'))}
        actions={[{ label: t('common.retry'), onClick: () => proxyQuery.refetch(), primary: true }]}
      />
    );
  } else {
    playable = true;
  }

  return (
    <div
      ref={setContainer}
      className={`dark-scope overflow-hidden ${useMpv && playable ? '' : 'bg-black'} ${miniPlayer ? 'fixed inset-0 z-[1000]' : 'relative h-full w-full'}`}
    >
      {playable && sources && proxyQuery.data ? (
        <VideoPlayer
          key={`${anilistId}:${episodeNumber}:${category}:${
            localRequested ? LOCAL_PROVIDER : requestedProvider ?? AUTO_PROVIDER
          }:${localRequested ? localQuery.dataUpdatedAt : sourcesQuery.dataUpdatedAt}:${useMpv ? 'mpv' : 'web'}`}
          anilistId={anilistId}
          malId={malId}
          animeTitle={animeTitle || `Anime ${anilistId}` /* i18n-ignore: stored as the series title in history */}
          animeCover={animeCover}
          episodeNumber={episodeNumber}
          episodeTitle={episodeTitle}
          category={category}
          metaLine={metaLine}
          sources={sources}
          provider={localRequested ? LOCAL_PROVIDER : servingProvider ?? requestedProvider ?? AUTO_PROVIDER}
          providerName={
            localRequested ? localProviderLabel() : servingProvider ? providerLabel(servingProvider) : null
          }
          proxyBase={proxyQuery.data}
          loadKey={localRequested ? localQuery.dataUpdatedAt : sourcesQuery.dataUpdatedAt}
          autoplay={settings.autoplay || continuingRef.current}
          autoSkip={{ intro: settings.skip_intro, outro: settings.skip_outro }}
          externalPlayer={settings.external_player === 'vlc' ? 'vlc' : 'mpv'}
          engineKind={useMpv ? 'mpv' : 'web'}
          onEngineFail={handleMpvFail}
          openExternally={settings.player_engine === 'external'}
          previous={previousStep}
          next={nextStep}
          queuedCount={pendingQueue.length}
          onEnded={handleEnded}
          onBack={handleBack}
          onOpenEpisodes={openEpisodes}
          onOpenServers={openServers}
          onReload={handleReload}
          onTryOtherServers={handleTryOtherServers}
          extraErrorActions={localErrorActions}
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
            title={animeTitle || (detailsQuery.isError ? t('watch.anime') : t('common.loading'))}
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
          goTo({ anilistId, episode, category: null, label: t('common.episodeNumber', { number: episode }) })
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
              label: t('common.episodeNumber', { number: pendingQueue[0] }),
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
        streamFacts={seenStreams.current}
        onSelectServer={handleSelectServer}
        container={container}
      />
    </div>
  );
};
