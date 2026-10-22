import React, { useCallback, useEffect, useState, useMemo } from 'react';
import { Search, List, Grid, Menu, ListPlus, X, HardDrive } from 'lucide-react';
import { EpisodeItem } from './EpisodeItem';
import { EpisodeActionsMenu } from './EpisodeActionsMenu';
import type { MenuAction } from './EpisodeActionsMenu';
import { CustomSelect } from './CustomSelect';
import { AddToPlaylistModal } from './AddToPlaylistModal';
import { Button } from './ui/Button';
import { SegmentedControl } from './ui/SegmentedControl';
import { EmptyState } from './ui/EmptyState';
import { Spinner } from './ui/Spinner';
import type { EpisodeItem as EpisodeItemType, EpisodesResult } from '../types';
import { useSetEpisodesWatched, useWatchedEpisodes } from '../hooks/useLibrary';
import { useUIStore } from '../stores/uiStore';
import { errorMessage } from './ui/ErrorState';
import { useFillerEpisodes } from '../hooks/useProviders';
import { AUTO_PROVIDER, useSettings } from '../hooks/useSettings';
import { usePlugins, useStreamingProviders } from '../hooks/usePlugins';
import { useLanguagePrefs } from '../hooks/useLanguagePrefs';
import { groupByLanguage, pluginLanguages } from '../lib/languages';
import { NoPluginsNotice } from './NoPluginsNotice';
import { distinctEpisodeTitle } from '../hooks/watchProgress';
import { useStoredChoice } from '../hooks/useStoredChoice';
import { LOCAL_PROVIDER, localProviderLabel } from '../hooks/useLocalMedia';
import { t } from '../i18n';

type Category = 'sub' | 'dub';
const VIEW_MODES = ['compact', 'list', 'grid'] as const;
type ViewMode = (typeof VIEW_MODES)[number];

interface EpisodeBrowserProps {
  anilistId: number;
  malId?: number | null;
  animeTitle: string;
  animeCover: string | null;
  /** Released episodes from the catalog (AniList / MAL); used when a provider has no list of its own. */
  catalogEpisodes?: EpisodeItemType[];
  isCatalogLoading?: boolean;
  /** Per-provider episode lists from `get_all_episodes`. */
  episodesData?: EpisodesResult;
  isProvidersLoading?: boolean;
  isProvidersError?: boolean;
  /** Initial audio and provider (from settings); the user's own choice overrides them. */
  defaultCategory?: Category;
  defaultProvider?: string;
  /** Saved position per episode number, 0..100. */
  progressByEpisode?: Record<number, number>;
  /** Episode numbers with a file of the user's ("Your files"); they play from the file. */
  localEpisodes?: number[];
  onSelectEpisode: (ep: EpisodeItemType, category: Category, provider: string) => void;
  /** The audio the user picked, when the parent keeps it (the detail page's sticky bar shares it). */
  chosenCategory?: Category | null;
  onChosenCategoryChange?: (category: Category) => void;
  /** Told whether a dub can be picked, so a copy of the switch elsewhere matches. */
  onDubAvailableChange?: (available: boolean) => void;
  /** The episode Play continues with; its card is outlined. */
  currentEpisode?: number | null;
}

const CHUNK_SIZE = 100;

// Stable defaults, so memoised values do not recompute on every render.
const NO_EPISODES: EpisodeItemType[] = [];
const NO_PROVIDERS: EpisodesResult = {};
const NO_PROGRESS: Record<number, number> = {};
const NO_LOCAL: number[] = [];

/** `list` plus an entry for every local episode it lacks; only local ones when `onlyLocal`. */
function withLocalEpisodes(list: EpisodeItemType[], local: number[], onlyLocal: boolean): EpisodeItemType[] {
  if (local.length === 0) return list;
  const isLocal = (n: number) => local.some((l) => Math.abs(l - n) < 0.01);
  const missing = local
    .filter((n) => !list.some((ep) => Math.abs(ep.number - n) < 0.01))
    .map((n) => ({ number: n, pipe_id: `local-${n}`, title: null, image: null, filler: false }));
  if (missing.length === 0 && !onlyLocal) return list;
  const merged = [...(onlyLocal ? list.filter((ep) => isLocal(ep.number)) : list), ...missing];
  return merged.sort((a, b) => a.number - b.number);
}

const viewOptions = () => [
  { value: 'compact' as const, icon: <Menu aria-hidden="true" className="h-4 w-4" />, 'aria-label': t('episodeBrowser.compactRows'), title: t('episodeBrowser.compactRows') },
  { value: 'list' as const, icon: <List aria-hidden="true" className="h-4 w-4" />, 'aria-label': t('episodeBrowser.list'), title: t('episodeBrowser.list') },
  { value: 'grid' as const, icon: <Grid aria-hidden="true" className="h-4 w-4" />, 'aria-label': t('episodeBrowser.grid'), title: t('episodeBrowser.grid') },
];

const GRID_CLASS: Record<ViewMode, string> = {
  compact: 'flex flex-col',
  list: 'grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-x-4 gap-y-1',
  grid: 'grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-2',
};

/**
 * Fills in titles, thumbnails and synopses the catalog list lacks from whichever provider
 * lists carry them, matched by episode number. Nothing the catalog already has is replaced.
 */
function withProviderDetails(
  episodes: EpisodeItemType[],
  providerLists: EpisodeItemType[][]
): EpisodeItemType[] {
  const details = new Map<number, Pick<EpisodeItemType, 'title' | 'image' | 'synopsis'>>();
  for (const list of providerLists) {
    for (const ep of list) {
      const known = details.get(ep.number);
      const title = distinctEpisodeTitle(ep.number, ep.title);
      if (!title && !ep.image && !ep.synopsis) continue;
      details.set(ep.number, {
        title: known?.title ?? title,
        image: known?.image ?? ep.image ?? null,
        synopsis: known?.synopsis ?? ep.synopsis ?? null,
      });
    }
  }
  if (details.size === 0) return episodes;
  return episodes.map((ep) => {
    const extra = details.get(ep.number);
    if (!extra) return ep;
    const title = distinctEpisodeTitle(ep.number, ep.title) ?? extra.title ?? ep.title;
    const image = ep.image ?? extra.image ?? null;
    const synopsis = ep.synopsis ?? extra.synopsis ?? null;
    return title === ep.title && image === ep.image && synopsis === ep.synopsis
      ? ep
      : { ...ep, title, image, synopsis };
  });
}

function longest(lists: EpisodeItemType[][]): EpisodeItemType[] {
  return lists.reduce<EpisodeItemType[]>((best, list) => (list.length > best.length ? list : best), []);
}

export const EpisodeBrowser: React.FC<EpisodeBrowserProps> = ({
  anilistId,
  malId,
  animeTitle,
  animeCover,
  catalogEpisodes = NO_EPISODES,
  isCatalogLoading = false,
  episodesData = NO_PROVIDERS,
  isProvidersLoading = false,
  isProvidersError = false,
  defaultCategory = 'sub',
  defaultProvider = AUTO_PROVIDER,
  progressByEpisode = NO_PROGRESS,
  localEpisodes = NO_LOCAL,
  onSelectEpisode,
  chosenCategory: controlledCategory,
  onChosenCategoryChange,
  onDubAvailableChange,
  currentEpisode = null,
}) => {
  // null = the user has not chosen yet, so the default from settings applies.
  const [ownCategory, setOwnCategory] = useState<Category | null>(null);
  const chosenCategory = controlledCategory !== undefined ? controlledCategory : ownCategory;
  const setChosenCategory = (next: Category) => {
    setOwnCategory(next);
    onChosenCategoryChange?.(next);
  };
  const [chosenProvider, setChosenProvider] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedChunkIndex, setSelectedChunkIndex] = useState(0);
  const [viewMode, setViewMode] = useStoredChoice<ViewMode>('episode-layout', VIEW_MODES, 'list');
  const [playlistTargetEpisodes, setPlaylistTargetEpisodes] = useState<EpisodeItemType[] | null>(null);

  const { data: watchedEpisodes = [] } = useWatchedEpisodes(anilistId);
  const { data: fillerEpisodes = [] } = useFillerEpisodes(malId);
  const { label: providerLabel, none: noPlugins, installed: installedPlugins } = useStreamingProviders();
  const plugins = usePlugins().data;
  const languagePrefs = useLanguagePrefs();
  const blurUnwatched = useSettings().data?.blur_unwatched_art === true;

  // Every provider in the result is selectable. One that comes back with empty lists plays
  // from the catalog list.
  const providerNames = useMemo(
    () =>
      Object.keys(episodesData).sort((a, b) =>
        providerLabel(a).localeCompare(providerLabel(b), undefined, { sensitivity: 'base' })
      ),
    [episodesData, providerLabel]
  );

  const requestedProvider = chosenProvider ?? defaultProvider;
  const activeProvider = providerNames.includes(requestedProvider) ? requestedProvider : AUTO_PROVIDER;

  const { subEpisodes, dubEpisodes, dubAvailable } = useMemo(() => {
    const all = Object.values(episodesData);
    const own = activeProvider === AUTO_PROVIDER ? undefined : episodesData[activeProvider];
    const ownSub = own?.sub ?? [];
    const ownDub = own?.dub ?? [];

    if (ownSub.length > 0 || ownDub.length > 0) {
      return { subEpisodes: ownSub, dubEpisodes: ownDub, dubAvailable: ownDub.length > 0 };
    }

    // Auto, or a provider that is asked by episode number and lists nothing itself:
    // the catalog is the episode list.
    const anySub = longest(all.map((p) => p?.sub ?? []));
    const anyDub = longest(all.map((p) => p?.dub ?? []));
    const sub = withProviderDetails(
      catalogEpisodes.length > 0 ? catalogEpisodes : anySub,
      all.map((p) => p?.sub ?? [])
    );

    if (activeProvider !== AUTO_PROVIDER) {
      // Whether this provider has a dub is not known until it is asked.
      return { subEpisodes: sub, dubEpisodes: sub, dubAvailable: true };
    }
    if (anyDub.length > 0) {
      return { subEpisodes: sub, dubEpisodes: anyDub, dubAvailable: true };
    }
    // Nobody lists a dub. That only rules a dub out if every provider published a list;
    // one that did not (or no answer yet) may still have it.
    const someUnlisted =
      all.length === 0 || all.some((p) => !(p?.sub?.length || p?.dub?.length));
    return { subEpisodes: sub, dubEpisodes: someUnlisted ? sub : [], dubAvailable: someUnlisted };
  }, [episodesData, activeProvider, catalogEpisodes]);

  useEffect(() => {
    onDubAvailableChange?.(dubAvailable);
  }, [dubAvailable, onDubAvailableChange]);

  // The episode numbers the serving plugin lists under dub (in Auto, any plugin), for the Dub pill.
  const dubNumbers = useMemo(() => {
    const own = activeProvider === AUTO_PROVIDER ? undefined : episodesData[activeProvider];
    const lists = own ? [own.dub ?? []] : Object.values(episodesData).map((p) => p?.dub ?? []);
    return new Set(lists.flat().map((ep) => ep.number));
  }, [episodesData, activeProvider]);

  const requestedCategory = chosenCategory ?? defaultCategory;
  const category: Category = requestedCategory === 'dub' && dubAvailable ? 'dub' : 'sub';
  const localActive = chosenProvider === LOCAL_PROVIDER && localEpisodes.length > 0;
  const isLocalEpisode = (n: number) => localEpisodes.some((l) => Math.abs(l - n) < 0.01);
  const allEpisodes = useMemo(
    () => withLocalEpisodes(category === 'dub' ? dubEpisodes : subEpisodes, localEpisodes, localActive),
    [category, dubEpisodes, subEpisodes, localEpisodes, localActive]
  );

  // Providers grouped by the language they play this audio version in; one without that
  // version is listed under the language of its other one.
  const providerOptions = useMemo(() => {
    const byId = new Map((plugins ?? []).map((plugin) => [plugin.id, plugin]));
    const other: Category = category === 'dub' ? 'sub' : 'dub';
    const wanted = (c: Category) => (c === 'dub' ? languagePrefs.dub : languagePrefs.sub);
    const own = providerNames.filter((name) => pluginLanguages(byId.get(name), category).length > 0);
    const rest = providerNames.filter((name) => !own.includes(name));
    const groups = [
      ...groupByLanguage(own, (name) => pluginLanguages(byId.get(name), category), wanted(category), category),
      ...groupByLanguage(rest, (name) => pluginLanguages(byId.get(name), other), wanted(other), other),
    ];
    return groups.flatMap((group) =>
      group.items.map((name) => ({ value: name, label: providerLabel(name), group: group.label }))
    );
  }, [plugins, providerNames, category, languagePrefs, providerLabel]);

  const chunks = useMemo(() => {
    const list: { index: number; label: string }[] = [];
    for (let start = 0; start < allEpisodes.length; start += CHUNK_SIZE) {
      const first = allEpisodes[start];
      const last = allEpisodes[Math.min(start + CHUNK_SIZE, allEpisodes.length) - 1];
      list.push({ index: list.length, label: `${first.number}–${last.number}` });
    }
    return list;
  }, [allEpisodes]);

  // A shorter list (other audio, other provider) must never leave the range out of bounds.
  const chunkIndex = Math.min(selectedChunkIndex, Math.max(0, chunks.length - 1));
  const query = searchQuery.trim().toLowerCase();

  const visibleEpisodes = useMemo(() => {
    if (query) {
      return allEpisodes
        .filter(
          (ep) => ep.number.toString().includes(query) || (ep.title && ep.title.toLowerCase().includes(query))
        )
        .slice(0, CHUNK_SIZE);
    }
    const startIdx = chunkIndex * CHUNK_SIZE;
    return allEpisodes.slice(startIdx, startIdx + CHUNK_SIZE);
  }, [allEpisodes, query, chunkIndex]);

  const isLoadingList = allEpisodes.length === 0 && (isCatalogLoading || isProvidersLoading);

  // The episode action menu: mark everything up to an episode as watched, or take it back.
  const showToast = useUIStore((s) => s.showToast);
  const setWatched = useSetEpisodesWatched();
  const [actionsFor, setActionsFor] = useState<{
    episode: EpisodeItemType;
    at: { x: number; y: number };
    trigger: HTMLElement;
  } | null>(null);
  const closeActions = useCallback(() => setActionsFor(null), []);
  const isWatched = (n: number) => watchedEpisodes.some((we) => Math.abs(we - n) < 0.01);

  const markEpisodes = (numbers: number[], watched: boolean, done: string) => {
    if (numbers.length === 0) return;
    setWatched.mutate(
      { anilistId, title: animeTitle, cover: animeCover, episodes: numbers, watched },
      {
        onSuccess: () => showToast({ type: 'success', title: done }),
        onError: (err) =>
          showToast({ type: 'error', title: t('episodeBrowser.couldNotUpdateHistory'), message: errorMessage(err) }),
      }
    );
  };

  const episodeActions = (ep: EpisodeItemType): MenuAction[] => {
    const through = allEpisodes.filter((e) => e.number <= ep.number + 0.001).map((e) => e.number); // i18n-ignore
    const first = through.length > 0 ? Math.min(...through) : ep.number;
    const many = through.length > 1;
    const vars = { first, last: ep.number, episode: ep.number };
    const anyWatched = through.some(isWatched);
    const allWatched = through.length > 0 && through.every(isWatched);
    const actions: MenuAction[] = [
      {
        label: t(many ? 'episodeBrowser.markRangeWatched' : 'episodeBrowser.markEpisodeWatched', vars),
        disabled: allWatched || setWatched.isPending,
        onSelect: () =>
          markEpisodes(through, true, t(many ? 'episodeBrowser.markedRange' : 'episodeBrowser.markedEpisode', vars)),
      },
      {
        label: t(many ? 'episodeBrowser.unmarkRange' : 'episodeBrowser.unmarkEpisode', vars),
        disabled: !anyWatched || setWatched.isPending,
        onSelect: () =>
          markEpisodes(through, false, t(many ? 'episodeBrowser.unmarkedRange' : 'episodeBrowser.unmarkedEpisode', vars)),
      },
    ];
    if (many) {
      actions.push(
        isWatched(ep.number)
          ? {
              label: t('episodeBrowser.unmarkOnly', vars),
              onSelect: () => markEpisodes([ep.number], false, t('episodeBrowser.unmarkedEpisode', vars)),
            }
          : {
              label: t('episodeBrowser.markOnlyWatched', vars),
              onSelect: () => markEpisodes([ep.number], true, t('episodeBrowser.markedEpisode', vars)),
            }
      );
    }
    return actions;
  };

  return (
    <div className="flex w-full flex-col gap-3">
      {noPlugins && (
        <NoPluginsNotice
          installed={installedPlugins}
          detail={
            localEpisodes.length > 0
              ? t('episodeBrowser.noPluginsLocal')
              : t('episodeBrowser.noPluginsCatalog')
          }
        />
      )}

      <div className="flex flex-wrap items-center gap-2">
        <SegmentedControl
          aria-label={t('episodeBrowser.audio')}
          value={category}
          onChange={(next) => {
            setChosenCategory(next);
            setSelectedChunkIndex(0);
          }}
          options={[
            { value: 'sub', label: t('common.sub') },
            { value: 'dub', label: t('common.dub'), disabled: !dubAvailable, title: dubAvailable ? undefined : t('episodeBrowser.noDubListed') },
          ]}
        />

        {!noPlugins && providerNames.length > 0 && (
          <CustomSelect
            aria-label={t('episodeBrowser.provider')}
            value={activeProvider}
            onChange={(val) => {
              setChosenProvider(val);
              setSelectedChunkIndex(0);
            }}
            options={[
              { value: AUTO_PROVIDER, label: t('episodeBrowser.providerAuto') },
              ...providerOptions,
            ]}
            className="w-48"
          />
        )}

        {localEpisodes.length > 0 && (
          <Button
            variant={localActive ? 'secondary' : 'neutral'}
            aria-pressed={localActive}
            title={localActive ? t('episodeBrowser.showingOnlyFiles') : t('episodeBrowser.showOnlyFiles')}
            onClick={() => {
              setChosenProvider(localActive ? null : LOCAL_PROVIDER);
              setSelectedChunkIndex(0);
            }}
          >
            <HardDrive aria-hidden="true" className="h-4 w-4" />
            {localProviderLabel()}
          </Button>
        )}

        {!noPlugins && isProvidersLoading && (
          <span className="flex items-center gap-2 text-xs text-text-muted">
            <Spinner size="sm" label={t('episodeBrowser.checkingProviders')} />
            {t('episodeBrowser.checkingProvidersEllipsis')}
          </span>
        )}
        {!noPlugins && isProvidersError && (
          <span className="text-xs text-text-muted">
            {t('episodeBrowser.providerListsFailed')}
          </span>
        )}

        <div className="flex-1" />

        <div className="relative w-56">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute start-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
          />
          <input
            type="text"
            aria-label={t('episodeBrowser.searchEpisodes')}
            placeholder={t('episodeBrowser.searchEpisodes')}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-9 w-full rounded-md bg-bg-input ps-9 pe-9 text-[13px] text-text-primary placeholder:text-text-muted"
          />
          {searchQuery && (
            <button
              type="button"
              aria-label={t('episodeBrowser.clearEpisodeSearch')}
              onClick={() => setSearchQuery('')}
              className="absolute end-1 top-1/2 flex h-7 w-7 -translate-y-1/2 cursor-pointer items-center justify-center rounded-md text-text-muted transition-colors hover:text-text-primary"
            >
              <X aria-hidden="true" className="h-4 w-4" />
            </button>
          )}
        </div>

        <SegmentedControl aria-label={t('episodeBrowser.episodeLayout')} value={viewMode} onChange={setViewMode} options={viewOptions()} />

        <Button
          icon
          variant="ghost"
          aria-label={t('episodeBrowser.addListedToPlaylist')}
          title={t('episodeBrowser.addListedToPlaylist')}
          disabled={visibleEpisodes.length === 0}
          onClick={() => setPlaylistTargetEpisodes(visibleEpisodes)}
        >
          <ListPlus aria-hidden="true" className="h-4 w-4" />
        </Button>
      </div>

      {!query && chunks.length > 1 && (
        <SegmentedControl
          aria-label={t('episodeBrowser.episodeRange')}
          size="sm"
          // Wraps instead of scrolling sideways: every range stays in view and reachable
          // with a plain mouse wheel.
          className="max-w-full flex-wrap self-start"
          value={String(chunkIndex)}
          onChange={(next) => setSelectedChunkIndex(Number(next))}
          options={chunks.map((chunk) => ({ value: String(chunk.index), label: chunk.label }))}
        />
      )}

      {isLoadingList ? (
        <div aria-busy="true" className={GRID_CLASS.list}>
          {Array.from({ length: 6 }).map((_, i) => (
            <div key={i} className="h-20 rounded-md shimmer-placeholder" />
          ))}
        </div>
      ) : visibleEpisodes.length === 0 ? (
        <EmptyState
          className="py-12"
          title={query ? t('episodeBrowser.noEpisodesMatch') : t('episodeBrowser.noEpisodesYet')}
          description={
            query
              ? t('episodeBrowser.nothingMatches', { query: searchQuery.trim() })
              : t('episodeBrowser.noReleasedEpisodes')
          }
          action={
            query ? (
              <Button variant="ghost" onClick={() => setSearchQuery('')}>
                {t('episodeBrowser.clearSearch')}
              </Button>
            ) : undefined
          }
        />
      ) : (
        <div className={GRID_CLASS[viewMode]}>
          {visibleEpisodes.map((ep) => (
            <EpisodeItem
              key={`${ep.pipe_id}-${ep.number}`}
              episode={ep}
              isWatched={isWatched(ep.number)}
              progressPercent={progressByEpisode[ep.number]}
              isFiller={fillerEpisodes.includes(Math.round(ep.number))}
              isLocal={isLocalEpisode(ep.number)}
              isCurrent={currentEpisode !== null && Math.abs(ep.number - currentEpisode) < 0.01}
              // On the sub list it says a dub exists; on the dub list every episode is one.
              hasDub={category === 'sub' && dubNumbers.has(ep.number)}
              blurArt={blurUnwatched && !isWatched(ep.number)}
              viewMode={viewMode}
              onSelect={(selectedEp) =>
                onSelectEpisode(
                  selectedEp,
                  category,
                  // A file of the user's plays unless a streaming plugin was picked.
                  isLocalEpisode(selectedEp.number) && (localActive || activeProvider === AUTO_PROVIDER || noPlugins)
                    ? LOCAL_PROVIDER
                    : activeProvider
                )
              }
              onAddToPlaylist={(targetEp) => setPlaylistTargetEpisodes([targetEp])}
              onOpenActions={(targetEp, at, trigger) => setActionsFor({ episode: targetEp, at, trigger })}
            />
          ))}
        </div>
      )}

      {actionsFor && (
        <EpisodeActionsMenu
          label={t('episodeBrowser.episodeActions', { episode: actionsFor.episode.number })}
          at={actionsFor.at}
          actions={episodeActions(actionsFor.episode)}
          onClose={closeActions}
          returnFocusTo={actionsFor.trigger}
        />
      )}

      {playlistTargetEpisodes && (
        <AddToPlaylistModal
          isOpen
          onClose={() => setPlaylistTargetEpisodes(null)}
          anilistId={anilistId}
          malId={malId}
          seriesTitle={animeTitle}
          seriesCover={animeCover}
          category={category}
          episodes={playlistTargetEpisodes}
        />
      )}
    </div>
  );
};
