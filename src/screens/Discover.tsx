import React, { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { Search, ChevronLeft, ChevronRight, X, SearchX, Dices, History } from 'lucide-react';
import clsx from 'clsx';
import { AnimeCard } from '../components/AnimeCard';
import { SkeletonCard } from '../components/SkeletonCard';
import { CustomSelect } from '../components/CustomSelect';
import { Button } from '../components/ui/Button';
import { Chip } from '../components/ui/Chip';
import { EmptyState } from '../components/ui/EmptyState';
import { ErrorState } from '../components/ui/ErrorState';
import { Spinner } from '../components/ui/Spinner';
import { useSearchAnime } from '../hooks/useAniList';
import { useDebouncedValue } from '../hooks/useDebouncedValue';
import { useSearchHistory } from '../hooks/useSearchHistory';
import { useSettings } from '../hooks/useSettings';
import { useUIStore } from '../stores/uiStore';
import { errorMessage } from '../components/ui/ErrorState';
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

const FORMATS = [
  { label: 'All formats', value: '' },
  { label: 'TV', value: 'TV' },
  { label: 'Movie', value: 'MOVIE' },
  { label: 'TV short', value: 'TV_SHORT' },
  { label: 'OVA', value: 'OVA' },
  { label: 'ONA', value: 'ONA' },
  { label: 'Special', value: 'SPECIAL' },
];

const STATUSES = [
  { label: 'Any status', value: '' },
  { label: 'Releasing', value: 'RELEASING' },
  { label: 'Finished', value: 'FINISHED' },
  { label: 'Not yet released', value: 'NOT_YET_RELEASED' },
  { label: 'Cancelled', value: 'CANCELLED' },
];

const SORTS = [
  { label: 'Sort: Popularity', value: 'POPULARITY_DESC' },
  { label: 'Sort: Trending', value: 'TRENDING_DESC' },
  { label: 'Sort: Score', value: 'SCORE_DESC' },
  { label: 'Sort: Newest', value: 'START_DATE_DESC' },
  { label: 'Sort: Favorites', value: 'FAVOURITES_DESC' },
  { label: 'Sort: Title', value: 'TITLE_ROMAJI' },
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
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const showToast = useUIStore((s) => s.showToast);
  const { data: settings } = useSettings();
  const searchHistory = useSearchHistory();
  const [surprising, setSurprising] = useState(false);

  const urlQuery = searchParams.get('q') ?? '';
  const selectedGenres = searchParams.getAll('genre').filter((g) => GENRES.includes(g));
  const format = oneOf(searchParams.get('format'), FORMATS, '');
  const status = oneOf(searchParams.get('status'), STATUSES, '');
  const sort = oneOf(searchParams.get('sort'), SORTS, DEFAULT_SORT);
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
    },
    { keepPrevious: true }
  );

  const toggleGenre = (genre: string) =>
    updateParams((params) => {
      const current = params.getAll('genre');
      params.delete('genre');
      const next = current.includes(genre) ? current.filter((g) => g !== genre) : [...current, genre];
      next.forEach((g) => params.append('genre', g));
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

  const surpriseMe = async () => {
    setSurprising(true);
    try {
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
      const eligible = list.filter(
        (m) => m.status !== 'NOT_YET_RELEASED' && (allowAdult || !m.is_adult)
      );
      const wellRated = eligible.filter((m) => (m.average_score ?? 0) >= SURPRISE_MIN_SCORE);
      const pool = wellRated.length > 0 ? wellRated : eligible;
      if (pool.length === 0) {
        showToast({ type: 'info', title: 'Nothing to pick this time', message: 'Try again in a moment.' });
        return;
      }
      navigate(`/detail/${pool[Math.floor(Math.random() * pool.length)].id}`);
    } catch (err) {
      showToast({ type: 'error', title: 'Could not pick an anime', message: errorMessage(err) });
    } finally {
      setSurprising(false);
    }
  };

  const hasActiveFilters =
    !!input.trim() || !!urlQuery || selectedGenres.length > 0 || !!format || !!status || sort !== DEFAULT_SORT;

  const isTyping = input.trim() !== urlQuery;
  // Past the first page the pager always shows, so an empty or failed page still has a way back.
  const showPager = page > 1 || (!isError && animeList.length === PER_PAGE);

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-[200px] flex-1">
            <Search
              aria-hidden="true"
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
            />
            <input
              type="text"
              aria-label="Search anime by title"
              placeholder="Search by title"
              autoComplete="off"
              spellCheck={false}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && input.trim()) commitQuery(input);
              }}
              className="h-9 w-full rounded-md bg-bg-input pl-9 pr-9 text-[13px] text-text-primary placeholder:text-text-muted"
            />
            {input && (
              <button
                type="button"
                aria-label="Clear search"
                onClick={() => setInput('')}
                className="absolute right-1 top-1/2 flex h-7 w-7 -translate-y-1/2 cursor-pointer items-center justify-center rounded-md text-text-muted transition-colors hover:text-text-primary"
              >
                <X aria-hidden="true" className="h-4 w-4" />
              </button>
            )}
          </div>

          <CustomSelect
            aria-label="Format"
            className="w-36"
            value={format}
            onChange={(val) => setParam('format', val)}
            options={FORMATS}
          />
          <CustomSelect
            aria-label="Status"
            className="w-44"
            value={status}
            onChange={(val) => setParam('status', val)}
            options={STATUSES}
          />
          <CustomSelect
            aria-label="Sort order"
            className="w-44"
            value={sort}
            onChange={(val) => setParam('sort', val, DEFAULT_SORT)}
            options={SORTS}
          />

          <Button variant="neutral" onClick={surpriseMe} disabled={surprising} title="Open a random well-rated anime">
            {surprising ? <Spinner size="sm" /> : <Dices aria-hidden="true" className="h-4 w-4" />}
            Surprise me
          </Button>

          {hasActiveFilters && (
            <Button variant="ghost" onClick={clearFilters}>
              <X aria-hidden="true" className="h-4 w-4" />
              Reset
            </Button>
          )}
        </div>

        {!input.trim() && searchHistory.history.length > 0 && (
          <div role="group" aria-label="Recent searches" className="flex flex-wrap items-center gap-2">
            <History aria-hidden="true" className="h-4 w-4 text-text-muted" />
            {searchHistory.history.map((recent) => (
              <span
                key={recent}
                className="ui-chip inline-flex h-7 max-w-[16rem] items-center rounded-sm bg-bg-elevated text-xs text-text-secondary"
              >
                <button
                  type="button"
                  onClick={() => commitQuery(recent)}
                  title={`Search for ${recent}`}
                  className="h-full min-w-0 cursor-pointer truncate rounded-l-sm pl-3 pr-1.5 transition-colors hover:text-text-primary"
                >
                  {recent}
                </button>
                <button
                  type="button"
                  aria-label={`Remove “${recent}” from recent searches`}
                  onClick={() => searchHistory.remove(recent)}
                  className="flex h-full w-6 flex-shrink-0 cursor-pointer items-center justify-center rounded-r-sm text-text-muted transition-colors hover:bg-ink/10 hover:text-text-primary"
                >
                  <X aria-hidden="true" className="h-3 w-3" />
                </button>
              </span>
            ))}
            <Button variant="ghost" size="sm" onClick={searchHistory.clear}>
              Clear
            </Button>
          </div>
        )}

        <div role="group" aria-label="Genres" className="flex flex-wrap items-center gap-2">
          {GENRES.map((genre) => (
            <Chip key={genre} selected={selectedGenres.includes(genre)} onClick={() => toggleGenre(genre)}>
              {genre}
            </Chip>
          ))}
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
          title="Search failed"
          error={error}
          onRetry={() => refetch()}
          retryLabel={isFetching ? 'Retrying…' : 'Retry'}
        />
      ) : animeList.length === 0 ? (
        page > 1 ? (
          <EmptyState
            icon={<SearchX aria-hidden="true" className="h-8 w-8" />}
            title="No more results"
            description="That was the last page."
          />
        ) : (
          <EmptyState
            icon={<SearchX aria-hidden="true" className="h-8 w-8" />}
            title="No anime found"
            description="Nothing matches this search and these filters."
            action={
              hasActiveFilters ? (
                <Button variant="secondary" onClick={clearFilters}>
                  Clear filters
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
          {animeList.map((anime) => (
            <AnimeCard key={anime.id} media={anime} />
          ))}
        </div>
      )}

      {showPager && (
        <nav aria-label="Pages" className="flex items-center justify-center gap-4 pt-2">
          <Button
            variant="ghost"
            onClick={() => goToPage(page - 1)}
            disabled={page <= 1 || isPlaceholderData}
          >
            <ChevronLeft aria-hidden="true" className="h-4 w-4" />
            Previous
          </Button>

          <span className="flex min-w-[72px] items-center justify-center gap-2 text-xs tabular-nums text-text-muted">
            Page {page}
            {(isFetching || isTyping) && <Spinner size="sm" />}
          </span>

          <Button
            variant="ghost"
            onClick={() => goToPage(page + 1)}
            disabled={isError || animeList.length < PER_PAGE || isPlaceholderData}
          >
            Next
            <ChevronRight aria-hidden="true" className="h-4 w-4" />
          </Button>
        </nav>
      )}
    </div>
  );
};
