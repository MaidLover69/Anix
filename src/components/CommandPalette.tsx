import React, { useState, useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { Search, Home, Compass, Bookmark, Calendar, Settings as SettingsIcon, X, Palette, Puzzle, UserRound, Globe, History } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import clsx from 'clsx';
import { useSearchAnime } from '../hooks/useAniList';
import { useDebouncedValue } from '../hooks/useDebouncedValue';
import { useSearchHistory } from '../hooks/useSearchHistory';
import { ShimmerImage } from './ShimmerImage';
import { Modal } from './ui/Modal';
import { Spinner } from './ui/Spinner';
import { errorMessage } from './ui/ErrorState';
import { mediaFormatLabel } from './library/format';
import { t, type TKey } from '../i18n';
import type { Media } from '../types';

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
}

interface NavItem {
  id: string;
  title: TKey;
  subtitle: TKey;
  path: string;
  icon: LucideIcon;
}

const NAV_ITEMS: NavItem[] = [
  { id: 'nav-home', title: 'commandPalette.navHome', subtitle: 'commandPalette.navHomeHint', path: '/', icon: Home },
  { id: 'nav-discover', title: 'commandPalette.navDiscover', subtitle: 'commandPalette.navDiscoverHint', path: '/discover', icon: Compass },
  { id: 'nav-schedule', title: 'commandPalette.navSchedule', subtitle: 'commandPalette.navScheduleHint', path: '/schedule', icon: Calendar },
  { id: 'nav-library', title: 'commandPalette.navLibrary', subtitle: 'commandPalette.navLibraryHint', path: '/library', icon: Bookmark },
  { id: 'nav-settings', title: 'common.settings', subtitle: 'commandPalette.navSettingsHint', path: '/settings', icon: SettingsIcon },
  { id: 'nav-theme', title: 'commandPalette.navTheme', subtitle: 'commandPalette.navThemeHint', path: '/settings?section=appearance', icon: Palette },
  { id: 'nav-plugins', title: 'commandPalette.navPlugins', subtitle: 'commandPalette.navPluginsHint', path: '/settings?section=plugins', icon: Puzzle },
  { id: 'nav-accounts', title: 'commandPalette.navAccounts', subtitle: 'commandPalette.navAccountsHint', path: '/settings?section=account', icon: UserRound },
  { id: 'nav-dns', title: 'commandPalette.navDns', subtitle: 'commandPalette.navDnsHint', path: '/settings?section=network', icon: Globe },
];

const MIN_QUERY_LENGTH = 2;

const KBD_CLASS = 'rounded-sm bg-ink/10 px-1.5 py-0.5 text-[11px] font-medium text-text-muted';

function titleOf(anime: Media): string {
  return anime.title.english || anime.title.user_preferred || anime.title.romaji || t('commandPalette.untitled');
}

// Mounted only while the palette is open, so its state starts fresh every time.
const PaletteBody: React.FC<{ onClose: () => void }> = ({ onClose }) => {
  const navigate = useNavigate();
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const searchHistory = useSearchHistory();

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
  const navResults = NAV_ITEMS.map((item) => ({ ...item, title: t(item.title), subtitle: t(item.subtitle) })).filter(
    (item) => item.title.toLowerCase().includes(needle) || item.subtitle.toLowerCase().includes(needle)
  );

  // With an empty box, the recent searches (shared with Discover) come first.
  const recents = trimmed === '' ? searchHistory.history : [];
  const animeOffset = recents.length;
  const navOffset = animeOffset + animeResults.length;
  const totalItems = navOffset + navResults.length;
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
    if (idx < animeOffset) {
      const recent = recents[idx];
      searchHistory.record(recent);
      navigate(`/discover?q=${encodeURIComponent(recent)}`);
    } else if (idx < navOffset) {
      // Opening a result keeps its search in the recent list, as on Discover.
      searchHistory.record(debouncedQuery);
      navigate(`/detail/${animeResults[idx - animeOffset].id}`);
    } else {
      const item = navResults[idx - navOffset];
      if (!item) return;
      navigate(item.path);
    }
    onClose();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // Enter on the clear/remove buttons must press them, not open the highlighted option.
    if (!(e.target instanceof HTMLInputElement)) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (totalItems > 0) setSelectedIndex((activeIndex + 1) % totalItems);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (totalItems > 0) setSelectedIndex((activeIndex - 1 + totalItems) % totalItems);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      handleSelect(activeIndex);
    } else if (e.key === 'Delete' && activeIndex >= 0 && activeIndex < animeOffset) {
      e.preventDefault();
      searchHistory.remove(recents[activeIndex]);
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
          aria-label={t('commandPalette.searchLabel')}
          placeholder={t('commandPalette.searchPlaceholder')}
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="h-full min-w-0 flex-1 bg-transparent text-sm text-text-primary outline-none placeholder:text-text-muted focus-visible:outline-none"
        />
        {isSearching && <Spinner size="sm" label={t('commandPalette.searching')} className="text-text-muted" />}
        {query && (
          <button
            type="button"
            aria-label={t('commandPalette.clearSearch')}
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
        aria-label={t('commandPalette.results')}
        className="max-h-[min(60vh,420px)] overflow-y-auto p-2"
      >
        {recents.length > 0 && (
          <>
            <div className="px-3 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-text-muted">
              {t('commandPalette.recentSearches')}
            </div>
            {recents.map((recent, idx) => (
              <div
                key={recent}
                id={`command-palette-option-${idx}`}
                role="option"
                aria-selected={activeIndex === idx}
                data-index={idx}
                onClick={() => handleSelect(idx)}
                onMouseMove={() => setSelectedIndex(idx)}
                className={optionClass(activeIndex === idx)}
              >
                <History aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted" />
                <span className="min-w-0 flex-1 truncate text-[13px] text-text-primary">{recent}</span>
                {/* Mouse only: from the keyboard, Delete removes the highlighted search. */}
                <button
                  type="button"
                  tabIndex={-1}
                  aria-label={t('commandPalette.removeRecent', { query: recent })}
                  onClick={(e) => {
                    e.stopPropagation();
                    searchHistory.remove(recent);
                  }}
                  className="flex h-6 w-6 flex-shrink-0 cursor-pointer items-center justify-center rounded-sm text-text-muted transition-colors hover:bg-ink/10 hover:text-text-primary"
                >
                  <X aria-hidden="true" className="h-3.5 w-3.5" />
                </button>
              </div>
            ))}
          </>
        )}

        {animeResults.length > 0 && (
          <>
            <div className="px-3 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-text-muted">
              {t('commandPalette.anime')}
            </div>
            {animeResults.map((anime, animeIdx) => {
              const idx = animeOffset + animeIdx;
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
              {t('commandPalette.goTo')}
            </div>
            {navResults.map((item, navIdx) => {
              const idx = navOffset + navIdx;
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
            {t('commandPalette.searchFailed', { error: errorMessage(error) })}
          </p>
        )}

        {!isError && !isSearching && totalItems === 0 && (
          <p className="px-3 py-8 text-center text-[13px] text-text-muted">
            {t('commandPalette.noResults', { query: trimmed })}
          </p>
        )}
      </div>

      <div className="flex items-center gap-4 border-t border-border px-4 py-2 text-xs text-text-muted">
        <span className="flex items-center gap-1.5">
          <kbd className={KBD_CLASS}>↑↓</kbd> {t('commandPalette.navigate')}
        </span>
        <span className="flex items-center gap-1.5">
          <kbd className={KBD_CLASS}>↵</kbd> {t('commandPalette.open')}
        </span>
        <span className="flex items-center gap-1.5">
          <kbd className={KBD_CLASS}>Esc</kbd> {t('common.close')}
        </span>
        {activeIndex >= 0 && activeIndex < animeOffset && (
          <span className="flex items-center gap-1.5">
            <kbd className={KBD_CLASS}>Del</kbd> {t('common.remove')} {/* i18n-ignore: key name */}
          </span>
        )}
        {recents.length > 0 && (
          <button
            type="button"
            onClick={searchHistory.clear}
            className="ms-auto cursor-pointer rounded-sm px-1.5 py-0.5 text-xs text-text-muted transition-colors hover:bg-ink/10 hover:text-text-primary"
          >
            {t('commandPalette.clearRecent')}
          </button>
        )}
      </div>
    </div>
  );
};

export const CommandPalette: React.FC<CommandPaletteProps> = ({ isOpen, onClose }) => (
  <Modal
    isOpen={isOpen}
    onClose={onClose}
    title={t('common.search')}
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
