import React, { useState, useMemo } from 'react';
import { Search, List, Grid, Menu, ListPlus, X } from 'lucide-react';
import { EpisodeItem } from './EpisodeItem';
import { CustomSelect } from './CustomSelect';
import { AddToPlaylistModal } from './AddToPlaylistModal';
import { Button } from './ui/Button';
import { SegmentedControl } from './ui/SegmentedControl';
import { EmptyState } from './ui/EmptyState';
import { Spinner } from './ui/Spinner';
import type { EpisodeItem as EpisodeItemType, EpisodesResult } from '../types';
import { useWatchedEpisodes } from '../hooks/useLibrary';
import { useFillerEpisodes } from '../hooks/useProviders';
import { AUTO_PROVIDER } from '../hooks/useSettings';
import { useStreamingProviders } from '../hooks/usePlugins';
import { NoPluginsNotice } from './NoPluginsNotice';
import { distinctEpisodeTitle } from '../hooks/watchProgress';

type Category = 'sub' | 'dub';
type ViewMode = 'compact' | 'list' | 'grid';

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
  onSelectEpisode: (ep: EpisodeItemType, category: Category, provider: string) => void;
}

const CHUNK_SIZE = 100;

// Stable defaults, so memoised values do not recompute on every render.
const NO_EPISODES: EpisodeItemType[] = [];
const NO_PROVIDERS: EpisodesResult = {};
const NO_PROGRESS: Record<number, number> = {};

const VIEW_OPTIONS = [
  { value: 'compact' as const, icon: <Menu aria-hidden="true" className="h-4 w-4" />, 'aria-label': 'Compact rows', title: 'Compact rows' },
  { value: 'list' as const, icon: <List aria-hidden="true" className="h-4 w-4" />, 'aria-label': 'List', title: 'List' },
  { value: 'grid' as const, icon: <Grid aria-hidden="true" className="h-4 w-4" />, 'aria-label': 'Grid', title: 'Grid' },
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
  onSelectEpisode,
}) => {
  // null = the user has not chosen yet, so the default from settings applies.
  const [chosenCategory, setChosenCategory] = useState<Category | null>(null);
  const [chosenProvider, setChosenProvider] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedChunkIndex, setSelectedChunkIndex] = useState(0);
  const [viewMode, setViewMode] = useState<ViewMode>('list');
  const [playlistTargetEpisodes, setPlaylistTargetEpisodes] = useState<EpisodeItemType[] | null>(null);

  const { data: watchedEpisodes = [] } = useWatchedEpisodes(anilistId);
  const { data: fillerEpisodes = [] } = useFillerEpisodes(malId);
  const { label: providerLabel, none: noPlugins, installed: installedPlugins } = useStreamingProviders();

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

  const requestedCategory = chosenCategory ?? defaultCategory;
  const category: Category = requestedCategory === 'dub' && dubAvailable ? 'dub' : 'sub';
  const allEpisodes = category === 'dub' ? dubEpisodes : subEpisodes;

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

  return (
    <div className="flex w-full flex-col gap-3">
      {noPlugins && (
        <NoPluginsNotice
          installed={installedPlugins}
          detail="The episodes below come from the catalog. Playing them needs a streaming plugin, which you add in Settings."
        />
      )}

      <div className="flex flex-wrap items-center gap-2">
        <SegmentedControl
          aria-label="Audio"
          value={category}
          onChange={(next) => {
            setChosenCategory(next);
            setSelectedChunkIndex(0);
          }}
          options={[
            { value: 'sub', label: 'Sub' },
            { value: 'dub', label: 'Dub', disabled: !dubAvailable, title: dubAvailable ? undefined : 'No dub listed' },
          ]}
        />

        {!noPlugins && providerNames.length > 0 && (
          <CustomSelect
            aria-label="Provider"
            value={activeProvider}
            onChange={(val) => {
              setChosenProvider(val);
              setSelectedChunkIndex(0);
            }}
            options={[
              { value: AUTO_PROVIDER, label: 'Provider: Auto' },
              ...providerNames.map((name) => ({ value: name, label: providerLabel(name) })),
            ]}
            className="w-48"
          />
        )}

        {!noPlugins && isProvidersLoading && (
          <span className="flex items-center gap-2 text-xs text-text-muted">
            <Spinner size="sm" label="Checking providers" />
            Checking providers…
          </span>
        )}
        {!noPlugins && isProvidersError && (
          <span className="text-xs text-text-muted">
            Provider lists could not be loaded; Auto still tries every provider.
          </span>
        )}

        <div className="flex-1" />

        <div className="relative w-56">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
          />
          <input
            type="text"
            aria-label="Search episodes"
            placeholder="Search episodes"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="h-9 w-full rounded-md bg-bg-input pl-9 pr-9 text-[13px] text-text-primary placeholder:text-text-muted"
          />
          {searchQuery && (
            <button
              type="button"
              aria-label="Clear episode search"
              onClick={() => setSearchQuery('')}
              className="absolute right-1 top-1/2 flex h-7 w-7 -translate-y-1/2 cursor-pointer items-center justify-center rounded-md text-text-muted transition-colors hover:text-text-primary"
            >
              <X aria-hidden="true" className="h-4 w-4" />
            </button>
          )}
        </div>

        <SegmentedControl aria-label="Episode layout" value={viewMode} onChange={setViewMode} options={VIEW_OPTIONS} />

        <Button
          icon
          variant="ghost"
          aria-label="Add the listed episodes to a playlist"
          title="Add the listed episodes to a playlist"
          disabled={visibleEpisodes.length === 0}
          onClick={() => setPlaylistTargetEpisodes(visibleEpisodes)}
        >
          <ListPlus aria-hidden="true" className="h-4 w-4" />
        </Button>
      </div>

      {!query && chunks.length > 1 && (
        <SegmentedControl
          aria-label="Episode range"
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
          title={query ? 'No episodes match' : 'No episodes yet'}
          description={
            query
              ? `Nothing matches "${searchQuery.trim()}" in this list.`
              : 'No released episodes were found for this title.'
          }
          action={
            query ? (
              <Button variant="ghost" onClick={() => setSearchQuery('')}>
                Clear search
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
              isWatched={watchedEpisodes.some((we) => Math.abs(we - ep.number) < 0.01)}
              progressPercent={progressByEpisode[ep.number]}
              isFiller={fillerEpisodes.includes(Math.round(ep.number))}
              viewMode={viewMode}
              onSelect={(selectedEp) => onSelectEpisode(selectedEp, category, activeProvider)}
              onAddToPlaylist={(targetEp) => setPlaylistTargetEpisodes([targetEp])}
            />
          ))}
        </div>
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
