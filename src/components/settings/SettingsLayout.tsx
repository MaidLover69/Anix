import React, { createContext, useContext, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { Link, useLocation, useSearchParams } from 'react-router-dom';
import { Bell, Cpu, Database, Download, Globe, Info, Languages, Palette, Play, Puzzle, Search, UserRound, X } from 'lucide-react';
import clsx from 'clsx';
import { useMediaQuery } from '../../hooks/useMediaQuery';
import { markParts } from '../../lib/textMarks';
import { t } from '../../i18n';
import { Meme, useHighModule, useMemeText, type MemeSpot } from '../meme/Meme';
import '../../theme/settings.css';

/**
 * Sections of the Settings screen, in page order. `/settings?section=<id>` opens one. A
 * section's id is its `id` prop, else its title in lower case. Add new sections here: the
 * side list shows only these, and an unlisted section is shown on every page.
 * Text is kept as keys, so it follows a language change (`t()` must not run at import).
 */
export const SETTINGS_SECTIONS = [
  { id: 'appearance', titleKey: 'settingsLayout.appearance', hintKey: 'settingsLayout.appearanceHint', icon: Palette },
  { id: 'playback', titleKey: 'settingsLayout.playback', hintKey: 'settingsLayout.playbackHint', icon: Play },
  { id: 'sources', titleKey: 'settingsLayout.sources', hintKey: 'settingsLayout.sourcesHint', icon: Languages },
  { id: 'downloads', titleKey: 'settingsLayout.downloads', hintKey: 'settingsLayout.downloadsHint', icon: Download },
  { id: 'system', titleKey: 'settingsLayout.system', hintKey: 'settingsLayout.systemHint', icon: Cpu },
  { id: 'network', titleKey: 'settingsLayout.network', hintKey: 'settingsLayout.networkHint', icon: Globe },
  { id: 'notifications', titleKey: 'settingsLayout.notifications', hintKey: 'settingsLayout.notificationsHint', icon: Bell },
  { id: 'account', titleKey: 'settingsLayout.account', hintKey: 'settingsLayout.accountHint', icon: UserRound },
  { id: 'data', titleKey: 'settingsLayout.data', hintKey: 'settingsLayout.dataHint', icon: Database },
  { id: 'about', titleKey: 'settingsLayout.about', hintKey: 'settingsLayout.aboutHint', icon: Info },
  { id: 'plugins', titleKey: 'settingsLayout.plugins', hintKey: 'settingsLayout.pluginsHint', icon: Puzzle },
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]['id'];

/** Meme mode › High: each listed section's meme (nav item and heading). */
const SECTION_MEME: Record<SettingsSectionId, MemeSpot> = {
  appearance: 'secAppearance',
  playback: 'secPlayback',
  sources: 'secSources',
  downloads: 'secDownloads',
  system: 'secSystem',
  network: 'secNetwork',
  notifications: 'secNotif',
  account: 'secAccount',
  data: 'secData',
  about: 'secAbout',
  plugins: 'secPlugins',
};

const isListed = (id: string): id is SettingsSectionId => SETTINGS_SECTIONS.some((s) => s.id === id);

/** Every word of `query` (already lower case) is in `text`. */
export function settingsMatch(text: string, query: string): boolean {
  const haystack = text.toLowerCase();
  return query.split(/\s+/).every((word) => haystack.includes(word));
}

interface SettingsView {
  /** The search, trimmed and in lower case; empty when not searching. */
  query: string;
  /** The open section, or null for every section (narrow list level, or tests). */
  active: SettingsSectionId | null;
}

const ViewContext = createContext<SettingsView>({ query: '', active: null });
/** True inside a section whose title matched the search: all its rows show. */
const SectionMatchContext = createContext(true);

/** `text` with the words of the settings search highlighted. */
const Marked: React.FC<{ text: string }> = ({ text }) => {
  const { query } = useContext(ViewContext);
  if (!query) return <>{text}</>;
  return (
    <>
      {markParts(text, query).map((part, index) =>
        part.hit ? (
          <mark key={index} className="rounded-sm bg-accent-500/30 text-inherit">
            {part.text}
          </mark>
        ) : (
          <React.Fragment key={index}>{part.text}</React.Fragment>
        )
      )}
    </>
  );
};

/**
 * The Settings screen frame: a search box, then the section list (name and one-line hint each)
 * beside the open section on wide windows, or above it as a scrolling row on narrow ones (960 px,
 * large text). While searching, every section and row that matches shows, the match marked.
 */
export const SettingsShell: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const searchId = useId();
  const [searchParams] = useSearchParams();
  const wide = useMediaQuery('(min-width: 1200px)');
  const [search, setSearch] = useState('');
  const memeText = useMemeText();
  const query = search.trim().toLowerCase();
  const linked = searchParams.get('section') ?? '';
  const active: SettingsSectionId = isListed(linked) ? linked : SETTINGS_SECTIONS[0].id;
  const paneRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const [anyMatch, setAnyMatch] = useState(true);
  // Narrow: bring the open section into view in the scrolling row, moving only the row (a
  // physical-pixel delta, so it also holds right to left).
  useEffect(() => {
    const list = listRef.current;
    const item = list?.querySelector('[aria-current="page"]');
    if (wide || !list || !item) return;
    const row = list.getBoundingClientRect();
    const box = item.getBoundingClientRect();
    if (box.left >= row.left && box.right <= row.right) return;
    list.scrollBy({ left: box.left - row.left - (row.width - box.width) / 2 });
  }, [wide, active, query]);
  // Runs after the sections have decided whether they match.
  useLayoutEffect(() => {
    setAnyMatch(!!paneRef.current?.querySelector('[data-settings-section]'));
  });

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="meme-title text-xl font-semibold text-text-primary">
          <Meme spot="setTitle" plain={t('common.settings')} />
        </h1>
        <div role="search" className="relative w-full max-w-xs">
          <label htmlFor={searchId} className="sr-only">
            {t('settingsLayout.searchSettings')}
          </label>
          <Search aria-hidden="true" className="pointer-events-none absolute start-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
          <input
            id={searchId}
            type="search"
            autoComplete="off"
            spellCheck={false}
            placeholder={memeText('setSearch', t('settingsLayout.searchSettings'))}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape' && search) {
                e.stopPropagation();
                setSearch('');
              }
            }}
            className="h-9 w-full rounded-md border border-border bg-bg-input ps-9 pe-9 text-[13px] text-text-primary placeholder:text-text-muted select-text [&::-webkit-search-cancel-button]:hidden"
          />
          {search && (
            <button
              type="button"
              aria-label={t('settingsLayout.clearSearch')}
              onClick={() => setSearch('')}
              className="absolute end-1.5 top-1/2 flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded-sm text-text-muted hover:bg-ink/10 hover:text-text-primary"
            >
              <X aria-hidden="true" className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </div>

      <div className={clsx('flex', wide ? 'items-start gap-8' : 'flex-col gap-4')}>
        <nav
          aria-label={t('settingsLayout.sectionsNav')}
          className={clsx('meme-set-nav', wide ? 'sticky top-0 w-60 flex-shrink-0' : 'w-full')}
        >
          {/* p-1 narrow: the scrolling row would clip the focus outline. */}
          <ul ref={listRef} className={clsx('flex', wide ? 'flex-col gap-0.5' : 'gap-1 overflow-x-auto p-1')}>
            {SETTINGS_SECTIONS.map((section) => {
              const current = !query && section.id === active;
              const Icon = section.icon;
              return (
                <li key={section.id} className={clsx(!wide && 'flex-none')}>
                  <Link
                    to={`?section=${section.id}`}
                    replace
                    onClick={() => setSearch('')}
                    aria-current={current ? 'page' : undefined}
                    className={clsx(
                      'flex items-start gap-3 rounded-lg px-3 py-2 transition-colors',
                      !wide && 'max-w-[15rem]',
                      current ? 'bg-accent-tint text-text-primary' : 'text-text-secondary hover:bg-ink/5 hover:text-text-primary'
                    )}
                  >
                    <Icon aria-hidden="true" className={clsx('mt-0.5 h-4 w-4 flex-shrink-0', current && 'text-accent-300')} />
                    <span className="min-w-0">
                      <span className="block truncate text-[13px] font-semibold">
                        <Meme spot={SECTION_MEME[section.id]} plain={t(section.titleKey)} />
                      </span>
                      <span className="block truncate text-xs text-text-muted">{t(section.hintKey)}</span>
                    </span>
                  </Link>
                </li>
              );
            })}
          </ul>
        </nav>

        <div ref={paneRef} className="flex min-w-0 max-w-2xl flex-1 flex-col gap-8">
          <ViewContext.Provider value={{ query, active: query ? null : active }}>{children}</ViewContext.Provider>
          {query && !anyMatch && (
            <p role="status" className="py-10 text-center text-[13px] text-text-muted">
              {t('settingsLayout.noMatch', { query: search.trim() })}
            </p>
          )}
        </div>
      </div>
    </div>
  );
};

export const Section: React.FC<{
  title: string;
  children: React.ReactNode;
  /** Makes the section linkable: it takes focus when `?section=<id>` is followed. */
  id?: SettingsSectionId;
}> = ({ title, children, id }) => {
  const titleId = useId();
  const sectionRef = useRef<HTMLElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const [searchParams] = useSearchParams();
  const linkedTo = id !== undefined && searchParams.get('section') === id;
  const { key } = useLocation();
  const view = useContext(ViewContext);
  const sectionKey = id ?? title.toLowerCase();
  const listed = SETTINGS_SECTIONS.find((s) => s.id === sectionKey);
  const titleMatches = !view.query || settingsMatch(title, view.query);
  const [rowMatches, setRowMatches] = useState(true);
  const high = useHighModule();

  // Rows that do not match render nothing; what is left decides whether the section shows.
  useLayoutEffect(() => {
    setRowMatches(titleMatches || !!cardRef.current?.querySelector('[data-settings-match]'));
  });

  const hidden = view.query
    ? !titleMatches && !rowMatches
    : view.active !== null && listed !== undefined && sectionKey !== view.active;

  // Runs after the layout has put a newly opened screen at the top; the location key makes a
  // second click on the same link scroll again.
  useEffect(() => {
    const section = sectionRef.current;
    if (!linkedTo || !section) return;
    section.focus({ preventScroll: true });
    section.scrollIntoView({ block: 'start' });
  }, [linkedTo, key]);

  return (
    <section
      ref={sectionRef}
      aria-labelledby={titleId}
      tabIndex={id ? -1 : undefined}
      data-settings-section={hidden ? undefined : ''}
      className={clsx(
        hidden ? 'hidden' : 'flex',
        'scroll-mt-4 flex-col gap-2 rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-accent-500/60 focus-visible:ring-offset-4 focus-visible:ring-offset-bg-page'
      )}
    >
      <h2 id={titleId} className="mb-1 text-base font-semibold text-text-primary">
        {high && listed && !view.query ? <Meme spot={SECTION_MEME[listed.id]} plain={title} /> : <Marked text={title} />}
      </h2>
      <div
        ref={cardRef}
        className={clsx(
          'ui-settings-rows',
          // While only some rows match, content that is not a matching row is left out too.
          view.query && !titleMatches && '[&>*:not([data-settings-match]):not(:has([data-settings-match]))]:hidden'
        )}
      >
        <SectionMatchContext.Provider value={titleMatches}>{children}</SectionMatchContext.Provider>
      </div>
    </section>
  );
};

/** A labelled row; `children` receives the ids to tie the control to its label and hint. */
export const Row: React.FC<{
  label: string;
  hint?: string;
  /** More words the settings search finds this row by. */
  keywords?: string;
  /** Puts the control under the label, for one that needs the full width. */
  stacked?: boolean;
  /** Meme mode › High: the label's meme; the search finds the row by it too. */
  meme?: MemeSpot;
  children: (ids: { labelId: string; hintId: string | undefined }) => React.ReactNode;
}> = ({ label, hint, keywords, stacked = false, meme, children }) => {
  const labelId = useId();
  const hintId = useId();
  const { query } = useContext(ViewContext);
  const sectionMatches = useContext(SectionMatchContext);
  const high = useHighModule();
  const memeWords = high && meme ? high.memeText(meme, label) : '';
  if (query && !sectionMatches && !settingsMatch(`${label} ${hint ?? ''} ${keywords ?? ''} ${memeWords}`, query)) return null;
  return (
    <div
      data-settings-match={query ? '' : undefined}
      className={
        stacked
          ? 'flex flex-col gap-3 py-3'
          : 'flex min-h-[56px] items-center justify-between gap-6 py-3'
      }
    >
      <div className="flex min-w-0 flex-col">
        <span id={labelId} className="text-[13px] font-medium text-text-primary">
          {memeWords && meme && !query ? <Meme spot={meme} plain={label} /> : <Marked text={label} />}
        </span>
        {hint && (
          <span id={hintId} className="mt-0.5 text-xs text-text-muted">
            {hint}
          </span>
        )}
      </div>
      <div className={stacked ? 'flex w-full' : 'flex flex-shrink-0 items-center'}>
        {children({ labelId, hintId: hint ? hintId : undefined })}
      </div>
    </div>
  );
};
