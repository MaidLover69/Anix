import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Search, ChevronLeft, ChevronRight, X, SearchX, Dices, History, Minus, Plus, EyeOff } from 'lucide-react';
import clsx from 'clsx';
import { AnimeCard } from '../components/AnimeCard';
import { SkeletonCard } from '../components/SkeletonCard';
import { CustomSelect } from '../components/CustomSelect';
import { Button } from '../components/ui/Button';
import { Chip } from '../components/ui/Chip';
import { EmptyState } from '../components/ui/EmptyState';
import { ErrorState } from '../components/ui/ErrorState';
import { SurpriseCard } from '../components/SurpriseCard';
import { useSearchAnime } from '../hooks/useAniList';
import { useDebouncedValue } from '../hooks/useDebouncedValue';
import { useSearchHistory } from '../hooks/useSearchHistory';
import { useWatchedHistory, useWatchlist } from '../hooks/useLibrary';
import { Spinner } from '../components/ui/Spinner';
import { mediaFormatLabel } from '../components/library/format';
import { t, type TKey } from '../i18n';
import { usePlayful, useSettings } from '../hooks/useSettings';
import { useOfflineStatus } from '../hooks/useSystem';
import type { SearchParams } from '../hooks/useAniList';
import type { Media } from '../types';

const GENRES = [
  'Action',
  'Adventure',
  'Comedy',
  'Drama',
  'Fantasy',
  'Horror',
  'Mahou Shoujo',
  'Mecha',
  'Music',
  'Mystery',
  'Psychological',
  'Romance',
  'Sci-Fi',
  'Slice of Life',
  'Sports',
  'Supernatural',
  'Thriller',
];

// The genre values above go to AniList as they are; these are only the shown names.
const GENRE_LABELS: Record<string, TKey> = {
  Action: 'discover.genreAction',
  Adventure: 'discover.genreAdventure',
  Comedy: 'discover.genreComedy',
  Drama: 'discover.genreDrama',
  Fantasy: 'discover.genreFantasy',
  Horror: 'discover.genreHorror',
  'Mahou Shoujo': 'discover.genreMahouShoujo',
  Mecha: 'discover.genreMecha',
  Music: 'discover.genreMusic',
  Mystery: 'discover.genreMystery',
  Psychological: 'discover.genrePsychological',
  Romance: 'discover.genreRomance',
  'Sci-Fi': 'discover.genreSciFi',
  'Slice of Life': 'discover.genreSliceOfLife',
  Sports: 'discover.genreSports',
  Supernatural: 'discover.genreSupernatural',
  Thriller: 'discover.genreThriller',
};

// Option lists are functions so their labels follow the current interface language.
const FORMATS = () => [
  { label: t('discover.allFormats'), value: '' },
  ...['TV', 'MOVIE', 'TV_SHORT', 'OVA', 'ONA', 'SPECIAL'].map((value) => ({ label: mediaFormatLabel(value), value })),
];

const STATUSES = () => [
  { label: t('discover.anyStatus'), value: '' },
  { label: t('discover.statusReleasing'), value: 'RELEASING' },
  { label: t('discover.statusFinished'), value: 'FINISHED' },
  { label: t('discover.statusNotYetReleased'), value: 'NOT_YET_RELEASED' },
  { label: t('discover.statusCancelled'), value: 'CANCELLED' },
];

const SORTS = () => [
  { label: t('discover.sortPopularity'), value: 'POPULARITY_DESC' },
  { label: t('discover.sortTrending'), value: 'TRENDING_DESC' },
  { label: t('discover.sortScore'), value: 'SCORE_DESC' },
  { label: t('discover.sortNewest'), value: 'START_DATE_DESC' },
  { label: t('discover.sortFavorites'), value: 'FAVOURITES_DESC' },
  { label: t('discover.sortTitle'), value: 'TITLE_ROMAJI' },
];

const MIN_SCORES = () => [
  { label: t('discover.anyScore'), value: '' },
  ...[90, 80, 70, 60, 50].map((n) => ({ label: t('discover.scoreAtLeast', { score: n }), value: String(n) })),
];

const SEASONS = () => [
  { label: t('discover.anySeason'), value: '' },
  { label: t('discover.seasonWinter'), value: 'WINTER' },
  { label: t('discover.seasonSpring'), value: 'SPRING' },
  { label: t('discover.seasonSummer'), value: 'SUMMER' },
  { label: t('discover.seasonFall'), value: 'FALL' },
];

const FIRST_YEAR = 1970;
const YEARS = () => [
  { label: t('discover.anyYear'), value: '' },
  ...Array.from({ length: new Date().getFullYear() + 2 - FIRST_YEAR }, (_, i) => {
    const year = String(new Date().getFullYear() + 1 - i);
    return { label: year, value: year };
  }),
];

const DEFAULT_SORT = 'POPULARITY_DESC';
const PER_PAGE = 24;
const SEARCH_DEBOUNCE_MS = 350;
const GRID_CLASS = 'grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-4 gap-y-6';

// "Surprise me" draws from this many pages of the most popular titles, keeping well-rated ones.
const SURPRISE_PAGES = 10;
const SURPRISE_PER_PAGE = 50;
const SURPRISE_MIN_SCORE = 75;

function oneOf(value: string | null, options: { value: string }[], fallback: string): string {
  return value !== null && options.some((o) => o.value === value) ? value : fallback;
}

export const Discover: React.FC = () => {
  // The URL is the single source of truth for filters and page, so Back restores them.
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const { data: settings } = useSettings();
  const playful = usePlayful();
  const searchHistory = useSearchHistory();
  const [surpriseOpen, setSurpriseOpen] = useState(false);
  const [formats, statuses, sorts, minScores, seasons, years] = useMemo(
    () => [FORMATS(), STATUSES(), SORTS(), MIN_SCORES(), SEASONS(), YEARS()],
    []
  );

  const urlQuery = searchParams.get('q') ?? '';
  const selectedGenres = searchParams.getAll('genre').filter((g) => GENRES.includes(g));
  // A genre is either wanted or left out, never both.
  const excludedGenres = searchParams.getAll('xgenre').filter((g) => GENRES.includes(g) && !selectedGenres.includes(g));
  const minScore = oneOf(searchParams.get('score'), minScores, '');
  const year = oneOf(searchParams.get('year'), years, '');
  const season = oneOf(searchParams.get('season'), seasons, '');
  const hideWatched = searchParams.get('unwatched') === '1';
  const format = oneOf(searchParams.get('format'), formats, '');
  const status = oneOf(searchParams.get('status'), statuses, '');
  const sort = oneOf(searchParams.get('sort'), sorts, DEFAULT_SORT);
  const page = Math.max(1, parseInt(searchParams.get('page') ?? '1', 10) || 1);

  const updateParams = useCallback(
    (mutate: (params: URLSearchParams) => void, keepPage = false) => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          mutate(next);
          if (!keepPage) next.delete('page');
          return next;
        },
        // Filters edit the current history entry instead of adding one per change.
        { replace: true }
      );
    },
    [setSearchParams]
  );

  const setParam = (key: string, value: string, defaultValue = '') =>
    updateParams((params) => {
      if (value === defaultValue) params.delete(key);
      else params.set(key, value);
    });

  // The text box is local state; it reaches the URL (and the query) after a pause in typing.
  const [input, setInput] = useState(urlQuery);
  const debouncedInput = useDebouncedValue(input, SEARCH_DEBOUNCE_MS);
  const lastSyncedQuery = useRef(urlQuery);

  useEffect(() => {
    const trimmed = debouncedInput.trim();
    if (trimmed === lastSyncedQuery.current) return;
    lastSyncedQuery.current = trimmed;
    updateParams((params) => {
      if (trimmed) params.set('q', trimmed);
      else params.delete('q');
    });
  }, [debouncedInput, updateParams]);

  // Back/forward or Reset changed the URL: bring the text box in line.
  useEffect(() => {
    if (urlQuery === lastSyncedQuery.current) return;
    lastSyncedQuery.current = urlQuery;
    setInput(urlQuery);
  }, [urlQuery]);

  const {
    data: animeList = [],
    isLoading,
    isFetching,
    isError,
    error,
    isPlaceholderData,
    refetch,
  } = useSearchAnime(
    {
      query: urlQuery || undefined,
      genres: selectedGenres.length > 0 ? selectedGenres : undefined,
      format: format || undefined,
      status: status || undefined,
      sort,
      page,
      perPage: PER_PAGE,
      excludeGenres: excludedGenres.length > 0 ? excludedGenres : undefined,
      minScore: minScore ? Number(minScore) : undefined,
      year: year ? Number(year) : undefined,
      season: season || undefined,
    },
    { keepPrevious: true }
  );

  // "Hide watched": anything with a watched episode or marked Completed.
  const { data: watchedRows } = useWatchedHistory(hideWatched);
  const { data: watchlist } = useWatchlist();
  const watchedIds = useMemo(
    () =>
      new Set([
        ...(watchedRows ?? []).map((row) => row.anilist_id),
        ...(watchlist ?? []).filter((entry) => entry.status === 'COMPLETED').map((entry) => entry.anilist_id),
      ]),
    [watchedRows, watchlist]
  );
  const shown = hideWatched ? animeList.filter((anime) => !watchedIds.has(anime.id)) : animeList;

  // A genre chip goes off → included → left out → off.
  const cycleGenre = (genre: string) =>
    updateParams((params) => {
      const included = params.getAll('genre').filter((g) => g !== genre);
      const excluded = params.getAll('xgenre').filter((g) => g !== genre);
      if (selectedGenres.includes(genre)) excluded.push(genre);
      else if (!excludedGenres.includes(genre)) included.push(genre);
      params.delete('genre');
      params.delete('xgenre');
      included.forEach((g) => params.append('genre', g));
      excluded.forEach((g) => params.append('xgenre', g));
    });

  const clearFilters = () => {
    lastSyncedQuery.current = '';
    setInput('');
    setSearchParams(new URLSearchParams(), { replace: true });
  };

  const goToPage = (next: number) => {
    updateParams((params) => {
      if (next <= 1) params.delete('page');
      else params.set('page', String(next));
    }, true);
    document.querySelector('main')?.scrollTo({ top: 0 });
  };

  // Puts a query in the box and the URL at once (a recent search, or Enter).
  const commitQuery = (value: string) => {
    const trimmed = value.trim();
    setInput(value);
    lastSyncedQuery.current = trimmed;
    updateParams((params) => {
      if (trimmed) params.set('q', trimmed);
      else params.delete('q');
    });
    if (trimmed) searchHistory.record(trimmed);
  };

  // One batch of "Surprise me" picks: a random page of the most popular titles, well-rated ones first.
  const drawSurprise = useCallback(async (): Promise<Media[]> => {
    const params: SearchParams = {
      sort: 'POPULARITY_DESC',
      page: 1 + Math.floor(Math.random() * SURPRISE_PAGES),
      perPage: SURPRISE_PER_PAGE,
    };
    // Same key as useSearchAnime, so a page drawn before is served from the cache.
    const list = await queryClient.fetchQuery<Media[]>({
      queryKey: ['searchAnime', params],
      queryFn: () =>
        invoke<Media[]>('search_anime', {
          query: null,
          genres: null,
          format: null,
          status: null,
          sort: params.sort,
          page: params.page,
          perPage: params.perPage,
        }),
      staleTime: 30 * 60 * 1000,
    });
    // The backend already leaves adult titles out unless they are allowed in Settings.
    const allowAdult = settings?.hide_adult_content === false;
    const eligible = list.filter((m) => m.status !== 'NOT_YET_RELEASED' && (allowAdult || !m.is_adult));
    const wellRated = eligible.filter((m) => (m.average_score ?? 0) >= SURPRISE_MIN_SCORE);
    const pool = wellRated.length > 0 ? wellRated : eligible;
    if (pool.length === 0) throw new Error(t('discover.nothingToPick'));
    return pool;
  }, [queryClient, settings?.hide_adult_content]);

  const hasActiveFilters =
    !!input.trim() ||
    !!urlQuery ||
    selectedGenres.length > 0 ||
    excludedGenres.length > 0 ||
    !!format ||
    !!status ||
    !!minScore ||
    !!year ||
    !!season ||
    hideWatched ||
    sort !== DEFAULT_SORT;

  const isTyping = input.trim() !== urlQuery;
  // A fallback catalogue's pages can be short (smaller pages, results filtered afterwards), so
  // while one serves, any non-empty page may have a next one.
  const { fallback } = useOfflineStatus();
  // A fallback catalogue cannot search with these; it only narrows each page it returns.
  const anilistOnly = fallback ? (
    <span
      title={t('discover.anilistOnlyHint', { source: fallback })}
      className="rounded-sm bg-state-warning/15 px-1.5 py-0.5 text-[11px] font-medium text-state-warning"
    >
      {t('discover.anilistOnly')}
    </span>
  ) : null;
  const mayHaveNext = animeList.length >= PER_PAGE || (!!fallback && animeList.length > 0);
  // Past the first page the pager always shows, so an empty or failed page still has a way back.
  const showPager = page > 1 || (!isError && mayHaveNext);

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <h1 className="sr-only">{t('discover.title')}</h1>
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-[200px] flex-1">
            <Search
              aria-hidden="true"
              className="pointer-events-none absolute start-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
            />
            <input
              type="text"
              aria-label={t('discover.searchLabel')}
              placeholder={t('discover.searchPlaceholder')}
              autoComplete="off"
              spellCheck={false}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && input.trim()) commitQuery(input);
              }}
              className="h-9 w-full rounded-md bg-bg-input ps-9 pe-9 text-[13px] text-text-primary placeholder:text-text-muted"
            />
            {input && (
              <button
                type="button"
                aria-label={t('discover.clearSearch')}
                onClick={() => setInput('')}
                className="absolute end-1 top-1/2 flex h-7 w-7 -translate-y-1/2 cursor-pointer items-center justify-center rounded-md text-text-muted transition-colors hover:text-text-primary"
              >
                <X aria-hidden="true" className="h-4 w-4" />
              </button>
            )}
          </div>

          <CustomSelect
            aria-label={t('discover.format')}
            className="min-w-36 max-w-64"
            value={format}
            onChange={(val) => setParam('format', val)}
            options={formats}
          />
          <CustomSelect
            aria-label={t('discover.status')}
            className="min-w-44 max-w-64"
            value={status}
            onChange={(val) => setParam('status', val)}
            options={statuses}
          />
          <CustomSelect
            aria-label={t('discover.sortOrder')}
            className="min-w-44 max-w-64"
            value={sort}
            onChange={(val) => setParam('sort', val, DEFAULT_SORT)}
            options={sorts}
          />

          <Button variant="neutral" onClick={() => setSurpriseOpen(true)} title={t('discover.surpriseHint')}>
            <Dices aria-hidden="true" className="h-4 w-4" />
            {t('discover.surpriseMe')}
          </Button>

          {hasActiveFilters && (
            <Button variant="ghost" onClick={clearFilters}>
              <X aria-hidden="true" className="h-4 w-4" />
              {t('discover.reset')}
            </Button>
          )}
        </div>

        {!input.trim() && searchHistory.history.length > 0 && (
          <div role="group" aria-label={t('discover.recentSearches')} className="flex flex-wrap items-center gap-2">
            <History aria-hidden="true" className="h-4 w-4 text-text-muted" />
            {searchHistory.history.map((recent) => (
              <span
                key={recent}
                className="ui-chip inline-flex h-7 max-w-[16rem] items-center rounded-sm bg-bg-elevated text-xs text-text-secondary"
              >
                <button
                  type="button"
                  onClick={() => commitQuery(recent)}
                  title={t('discover.searchFor', { query: recent })}
                  className="h-full min-w-0 cursor-pointer truncate rounded-s-sm ps-3 pe-1.5 transition-colors hover:text-text-primary"
                >
                  {recent}
                </button>
                <button
                  type="button"
                  aria-label={t('discover.removeRecent', { query: recent })}
                  onClick={() => searchHistory.remove(recent)}
                  className="flex h-full w-6 flex-shrink-0 cursor-pointer items-center justify-center rounded-e-sm text-text-muted transition-colors hover:bg-ink/10 hover:text-text-primary"
                >
                  <X aria-hidden="true" className="h-3 w-3" />
                </button>
              </span>
            ))}
            <Button variant="ghost" size="sm" onClick={searchHistory.clear}>
              {t('discover.clear')}
            </Button>
          </div>
        )}

        <div role="group" aria-label={t('discover.moreFilters')} className="flex flex-wrap items-center gap-2">
          <CustomSelect aria-label={t('discover.minimumScore')} className="min-w-36 max-w-64" value={minScore} onChange={(val) => setParam('score', val)} options={minScores} />
          {minScore && anilistOnly}
          <CustomSelect aria-label={t('discover.year')} className="min-w-28 max-w-64" value={year} onChange={(val) => setParam('year', val)} options={years} />
          <CustomSelect aria-label={t('discover.season')} className="min-w-32 max-w-64" value={season} onChange={(val) => setParam('season', val)} options={seasons} />
          {(year || season) && anilistOnly}
          <Chip selected={hideWatched} onClick={() => setParam('unwatched', hideWatched ? '' : '1')} className="gap-1.5">
            <EyeOff aria-hidden="true" className="h-3.5 w-3.5" />
            {t('discover.hideWatched')}
          </Chip>
        </div>

        <div role="group" aria-label={t('discover.genresHint')} className="flex flex-wrap items-center gap-2">
          {GENRES.map((genre) => {
            const state = selectedGenres.includes(genre) ? 'include' : excludedGenres.includes(genre) ? 'exclude' : 'off';
            const label = t(GENRE_LABELS[genre]);
            return (
              <button
                key={genre}
                type="button"
                onClick={() => cycleGenre(genre)}
                aria-label={
                  state === 'off'
                    ? label
                    : t(state === 'include' ? 'discover.genreIncluded' : 'discover.genreLeftOut', { genre: label })
                }
                title={t(
                  state === 'off' ? 'discover.clickToInclude' : state === 'include' ? 'discover.clickToLeaveOut' : 'discover.clickToClear'
                )}
                className={clsx(
                  'ui-chip inline-flex h-7 cursor-pointer items-center gap-1 whitespace-nowrap rounded-sm px-3 text-xs transition-colors select-none',
                  state === 'include' && 'bg-accent-tint text-accent-300',
                  state === 'exclude' && 'bg-state-error/15 text-state-error line-through',
                  state === 'off' && 'bg-bg-elevated text-text-secondary hover:text-text-primary'
                )}
              >
                {state === 'include' && <Plus aria-hidden="true" className="h-3 w-3" />}
                {state === 'exclude' && <Minus aria-hidden="true" className="h-3 w-3" />}
                {label}
              </button>
            );
          })}
          {excludedGenres.length > 0 && anilistOnly}
        </div>
      </div>

      {isLoading ? (
        <div aria-busy="true" className={GRID_CLASS}>
          {Array.from({ length: PER_PAGE }).map((_, i) => (
            <SkeletonCard key={i} />
          ))}
        </div>
      ) : isError ? (
        <ErrorState
          title={t('discover.searchFailed')}
          error={error}
          onRetry={() => refetch()}
          retryLabel={isFetching ? t('discover.retrying') : t('common.retry')}
        />
      ) : shown.length === 0 ? (
        animeList.length > 0 ? (
          // "Hide watched" filters this page only; later pages can still have unwatched results.
          <EmptyState
            icon={<EyeOff aria-hidden="true" className="h-8 w-8" />}
            title={t('discover.allWatched')}
            description={t('discover.allWatchedHint')}
          />
        ) : page > 1 ? (
          <EmptyState
            icon={<SearchX aria-hidden="true" className="h-8 w-8" />}
            title={playful ? t('discover.endPlayful') : t('discover.noMoreResults')}
            description={playful ? t('discover.endPlayfulHint') : t('discover.lastPage')}
            kaomoji="(^_^)b"
          />
        ) : (
          <EmptyState
            icon={<SearchX aria-hidden="true" className="h-8 w-8" />}
            title={playful ? t('discover.noMatchPlayful') : t('discover.noAnimeFound')}
            description={playful ? t('discover.noMatchPlayfulHint') : t('discover.noMatchHint')}
            mood="confused"
            kaomoji="(o_O)?"
            action={
              hasActiveFilters ? (
                <Button variant="secondary" onClick={clearFilters}>
                  {t('discover.clearFilters')}
                </Button>
              ) : undefined
            }
          />
        )
      ) : (
        <div
          aria-busy={isPlaceholderData}
          // Opening a result keeps its search in the recent list.
          onClickCapture={(e) => {
            if (urlQuery && e.target instanceof Element && e.target.closest('a')) searchHistory.record(urlQuery);
          }}
          className={clsx(GRID_CLASS, 'transition-opacity', isPlaceholderData && 'opacity-50')}
        >
          {shown.map((anime) => (
            <AnimeCard key={anime.id} media={anime} />
          ))}
        </div>
      )}

      {showPager && (
        <nav aria-label={t('discover.pages')} className="flex items-center justify-center gap-4 pt-2">
          <Button
            variant="ghost"
            onClick={() => goToPage(page - 1)}
            disabled={page <= 1 || isPlaceholderData}
          >
            <ChevronLeft aria-hidden="true" className="h-4 w-4 rtl:-scale-x-100" />
            {t('discover.previous')}
          </Button>

          <span className="flex min-w-[72px] items-center justify-center gap-2 text-xs tabular-nums text-text-muted">
            {t('discover.page', { page })}
            {(isFetching || isTyping) && <Spinner size="sm" />}
          </span>

          <Button
            variant="ghost"
            onClick={() => goToPage(page + 1)}
            disabled={isError || !mayHaveNext || isPlaceholderData}
          >
            {t('discover.next')}
            <ChevronRight aria-hidden="true" className="h-4 w-4 rtl:-scale-x-100" />
          </Button>
        </nav>
      )}

      <SurpriseCard isOpen={surpriseOpen} onClose={() => setSurpriseOpen(false)} draw={drawSurprise} />
    </div>
  );
};
