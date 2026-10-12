import React, { useState, useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { Search, Home, Compass, Bookmark, Calendar, Settings as SettingsIcon, X } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import clsx from 'clsx';
import { useSearchAnime } from '../hooks/useAniList';
import { useDebouncedValue } from '../hooks/useDebouncedValue';
import { ShimmerImage } from './ShimmerImage';
import { Modal } from './ui/Modal';
import { Spinner } from './ui/Spinner';
import { errorMessage } from './ui/ErrorState';
import { mediaFormatLabel } from './library/format';
import type { Media } from '../types';

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
}

interface NavItem {
  id: string;
  title: string;
  subtitle: string;
  path: string;
  icon: LucideIcon;
}

const NAV_ITEMS: NavItem[] = [
  { id: 'nav-home', title: 'Home', subtitle: 'Trending, popular and continue watching', path: '/', icon: Home },
  { id: 'nav-discover', title: 'Discover', subtitle: 'Search and browse by genre, format and status', path: '/discover', icon: Compass },
  { id: 'nav-schedule', title: 'Schedule', subtitle: 'What airs this week', path: '/schedule', icon: Calendar },
  { id: 'nav-library', title: 'Library', subtitle: 'Watchlist, history, downloads and playlists', path: '/library', icon: Bookmark },
  { id: 'nav-settings', title: 'Settings', subtitle: 'Playback, plugins, network and account', path: '/settings', icon: SettingsIcon },
];

const MIN_QUERY_LENGTH = 2;

const KBD_CLASS = 'rounded-sm bg-ink/10 px-1.5 py-0.5 text-[11px] font-medium text-text-muted';

function titleOf(anime: Media): string {
  return anime.title.english || anime.title.user_preferred || anime.title.romaji || 'Untitled';
}

// Mounted only while the palette is open, so its state starts fresh every time.
const PaletteBody: React.FC<{ onClose: () => void }> = ({ onClose }) => {
  const navigate = useNavigate();
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const trimmed = query.trim();
  const debouncedQuery = useDebouncedValue(trimmed, 200);
  const searchEnabled = debouncedQuery.length >= MIN_QUERY_LENGTH;

  const { data, isFetching, isError, error } = useSearchAnime(
    { query: searchEnabled ? debouncedQuery : undefined, perPage: 6 },
    { enabled: searchEnabled }
  );

  // Results belong to the debounced query; hide them once the box is cleared.
  const animeResults: Media[] = trimmed.length >= MIN_QUERY_LENGTH && searchEnabled ? data ?? [] : [];
  const needle = trimmed.toLowerCase();
  const navResults = NAV_ITEMS.filter(
    (item) => item.title.toLowerCase().includes(needle) || item.subtitle.toLowerCase().includes(needle)
  );

  const totalItems = animeResults.length + navResults.length;
  const activeIndex = totalItems === 0 ? -1 : Math.min(selectedIndex, totalItems - 1);
  const isSearching = trimmed.length >= MIN_QUERY_LENGTH && (isFetching || trimmed !== debouncedQuery);

  useEffect(() => {
    setSelectedIndex(0);
  }, [trimmed, animeResults.length]);

  useEffect(() => {
    if (activeIndex < 0) return;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${activeIndex}"]`)
      ?.scrollIntoView({ block: 'nearest' });
  }, [activeIndex]);

  const handleSelect = (idx: number) => {
    if (idx < 0) return;
    if (idx < animeResults.length) {
      navigate(`/detail/${animeResults[idx].id}`);
    } else {
      const item = navResults[idx - animeResults.length];
      if (!item) return;
      navigate(item.path);
    }
    onClose();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (totalItems > 0) setSelectedIndex((activeIndex + 1) % totalItems);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (totalItems > 0) setSelectedIndex((activeIndex - 1 + totalItems) % totalItems);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      handleSelect(activeIndex);
    }
  };

  const optionClass = (selected: boolean) =>
    clsx(
      'flex cursor-pointer items-center gap-3 rounded-md px-3 py-2',
      selected ? 'bg-accent-tint text-text-primary' : 'text-text-secondary'
    );

  return (
    <div onKeyDown={handleKeyDown}>
      <div className="flex h-12 items-center gap-3 border-b border-border px-4">
        <Search aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted" />
        <input
          type="text"
          data-autofocus
          role="combobox"
          aria-expanded="true"
          aria-controls="command-palette-results"
          aria-activedescendant={activeIndex >= 0 ? `command-palette-option-${activeIndex}` : undefined}
          aria-label="Search anime or jump to a screen"
          placeholder="Search anime or jump to…"
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="h-full min-w-0 flex-1 bg-transparent text-sm text-text-primary outline-none placeholder:text-text-muted focus-visible:outline-none"
        />
        {isSearching && <Spinner size="sm" label="Searching" className="text-text-muted" />}
        {query && (
          <button
            type="button"
            aria-label="Clear search"
            onClick={() => setQuery('')}
            className="flex h-7 w-7 flex-shrink-0 cursor-pointer items-center justify-center rounded-md text-text-muted transition-colors hover:text-text-primary"
          >
            <X aria-hidden="true" className="h-4 w-4" />
          </button>
        )}
      </div>

      <div
        ref={listRef}
        id="command-palette-results"
        role="listbox"
        aria-label="Results"
        className="max-h-[min(60vh,420px)] overflow-y-auto p-2"
      >
        {animeResults.length > 0 && (
          <>
            <div className="px-3 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-text-muted">
              Anime
            </div>
            {animeResults.map((anime, idx) => {
              const title = titleOf(anime);
              const caption = [
                mediaFormatLabel(anime.format),
                anime.season_year,
                anime.average_score ? `${anime.average_score}%` : null,
                anime.genres.slice(0, 2).join(', '),
              ]
                .filter(Boolean)
                .join(' · ');
              return (
                <div
                  key={anime.id}
                  id={`command-palette-option-${idx}`}
                  role="option"
                  aria-selected={activeIndex === idx}
                  data-index={idx}
                  onClick={() => handleSelect(idx)}
                  onMouseMove={() => setSelectedIndex(idx)}
                  className={optionClass(activeIndex === idx)}
                >
                  <ShimmerImage
                    src={anime.cover_image.large || anime.cover_image.extra_large}
                    alt=""
                    className="h-12 w-8 flex-shrink-0 rounded-sm"
                  />
                  <div className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate text-[13px] font-medium text-text-primary">{title}</span>
                    {caption && <span className="truncate text-xs text-text-muted">{caption}</span>}
                  </div>
                </div>
              );
            })}
          </>
        )}

        {navResults.length > 0 && (
          <>
            <div className="px-3 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-text-muted">
              Go to
            </div>
            {navResults.map((item, navIdx) => {
              const idx = animeResults.length + navIdx;
              const Icon = item.icon;
              return (
                <div
                  key={item.id}
                  id={`command-palette-option-${idx}`}
                  role="option"
                  aria-selected={activeIndex === idx}
                  data-index={idx}
                  onClick={() => handleSelect(idx)}
                  onMouseMove={() => setSelectedIndex(idx)}
                  className={optionClass(activeIndex === idx)}
                >
                  <Icon aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted" />
                  <div className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate text-[13px] font-medium text-text-primary">{item.title}</span>
                    <span className="truncate text-xs text-text-muted">{item.subtitle}</span>
                  </div>
                </div>
              );
            })}
          </>
        )}

        {isError && trimmed.length >= MIN_QUERY_LENGTH && !isSearching && (
          <p role="alert" className="px-3 py-4 text-center text-xs text-state-error select-text">
            Search failed: {errorMessage(error)}
          </p>
        )}

        {!isError && !isSearching && totalItems === 0 && (
          <p className="px-3 py-8 text-center text-[13px] text-text-muted">
            No results for “{trimmed}”
          </p>
        )}
      </div>

      <div className="flex items-center gap-4 border-t border-border px-4 py-2 text-xs text-text-muted">
        <span className="flex items-center gap-1.5">
          <kbd className={KBD_CLASS}>↑↓</kbd> Navigate
        </span>
        <span className="flex items-center gap-1.5">
          <kbd className={KBD_CLASS}>↵</kbd> Open
        </span>
        <span className="flex items-center gap-1.5">
          <kbd className={KBD_CLASS}>Esc</kbd> Close
        </span>
      </div>
    </div>
  );
};

export const CommandPalette: React.FC<CommandPaletteProps> = ({ isOpen, onClose }) => (
  <Modal
    isOpen={isOpen}
    onClose={onClose}
    title="Search"
    size="xl"
    position="top"
    hideHeader
    padded={false}
    className="overflow-hidden"
  >
    <PaletteBody onClose={onClose} />
  </Modal>
);

export default CommandPalette;
