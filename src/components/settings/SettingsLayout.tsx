import React, { createContext, useContext, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { Link, useLocation, useSearchParams } from 'react-router-dom';
import { ArrowLeft, ChevronRight, Search, X } from 'lucide-react';
import clsx from 'clsx';
import { useMediaQuery } from '../../hooks/useMediaQuery';
import { t } from '../../i18n';

/**
 * Sections of the Settings screen, in page order. `/settings?section=<id>` opens one. A
 * section's id is its `id` prop, else its title in lower case. Add new sections here: the
 * list and the drill-down show only these, and an unlisted section is shown on every page.
 * Text is kept as keys, so it follows a language change (`t()` must not run at import).
 */
export const SETTINGS_SECTIONS = [
  { id: 'appearance', titleKey: 'settingsLayout.appearance', hintKey: 'settingsLayout.appearanceHint' },
  { id: 'playback', titleKey: 'settingsLayout.playback', hintKey: 'settingsLayout.playbackHint' },
  { id: 'sources', titleKey: 'settingsLayout.sources', hintKey: 'settingsLayout.sourcesHint' },
  { id: 'downloads', titleKey: 'settingsLayout.downloads', hintKey: 'settingsLayout.downloadsHint' },
  { id: 'system', titleKey: 'settingsLayout.system', hintKey: 'settingsLayout.systemHint' },
  { id: 'network', titleKey: 'settingsLayout.network', hintKey: 'settingsLayout.networkHint' },
  { id: 'notifications', titleKey: 'settingsLayout.notifications', hintKey: 'settingsLayout.notificationsHint' },
  { id: 'account', titleKey: 'settingsLayout.account', hintKey: 'settingsLayout.accountHint' },
  { id: 'data', titleKey: 'settingsLayout.data', hintKey: 'settingsLayout.dataHint' },
  { id: 'about', titleKey: 'settingsLayout.about', hintKey: 'settingsLayout.aboutHint' },
  { id: 'plugins', titleKey: 'settingsLayout.plugins', hintKey: 'settingsLayout.pluginsHint' },
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]['id'];

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

/**
 * The Settings screen frame: a search box, then the section list and the open section side by
 * side on wide windows, or the list that drills into one section (with Back) on narrow ones.
 * While searching, every section and row that matches shows.
 */
export const SettingsShell: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const searchId = useId();
  const [searchParams, setSearchParams] = useSearchParams();
  const wide = useMediaQuery('(min-width: 1200px)');
  const [search, setSearch] = useState('');
  const query = search.trim().toLowerCase();
  const linked = searchParams.get('section') ?? '';
  const active: SettingsSectionId | null = isListed(linked) ? linked : wide ? SETTINGS_SECTIONS[0].id : null;
  const showList = wide || (!active && !query);
  const activeTitleKey = SETTINGS_SECTIONS.find((s) => s.id === active)?.titleKey;
  const paneRef = useRef<HTMLDivElement>(null);
  const [anyMatch, setAnyMatch] = useState(true);
  // Runs after the sections have decided whether they match.
  useLayoutEffect(() => {
    setAnyMatch(!!paneRef.current?.querySelector('[data-settings-section]'));
  });

  return (
    <div className="flex flex-col gap-6 pb-16 animate-fade-in">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-xl font-semibold text-text-primary">{t('common.settings')}</h1>
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
            placeholder={t('settingsLayout.searchSettings')}
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

      <div className={clsx(wide && 'flex items-start gap-8')}>
        {showList && (
          <nav aria-label={t('settingsLayout.sectionsNav')} className={clsx(wide ? 'sticky top-0 w-56 flex-shrink-0' : 'w-full max-w-2xl')}>
            <ul className={clsx('flex flex-col', wide ? 'gap-0.5' : 'divide-y divide-border rounded-xl ui-card bg-bg-card')}>
              {SETTINGS_SECTIONS.map((section) => {
                const current = wide && !query && section.id === active;
                return (
                  <li key={section.id}>
                    <Link
                      to={`?section=${section.id}`}
                      replace={wide}
                      onClick={() => setSearch('')}
                      aria-current={current ? 'page' : undefined}
                      className={clsx(
                        'flex items-center gap-3 transition-colors',
                        wide
                          ? 'rounded-md px-3 py-1.5 text-[13px]'
                          : 'px-4 py-3 hover:bg-ink/5',
                        current
                          ? 'bg-accent-tint font-medium text-accent-300'
                          : wide
                            ? 'text-text-secondary hover:bg-ink/10 hover:text-text-primary'
                            : 'text-text-primary'
                      )}
                    >
                      <span className="min-w-0 flex-1">
                        <span className="block truncate">{t(section.titleKey)}</span>
                        {!wide && <span className="block truncate text-xs text-text-muted">{t(section.hintKey)}</span>}
                      </span>
                      {!wide && <ChevronRight aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted rtl:-scale-x-100" />}
                    </Link>
                  </li>
                );
              })}
            </ul>
          </nav>
        )}

        <div
          ref={paneRef}
          className={clsx(!wide && !active && !query ? 'hidden' : 'flex', 'min-w-0 max-w-2xl flex-1 flex-col gap-8')}
        >
          {!wide && active && !query && (
            <button
              type="button"
              onClick={() => setSearchParams({}, { replace: true })}
              className="-mb-4 flex w-fit items-center gap-1.5 rounded-md px-2 py-1 text-[13px] text-text-secondary hover:bg-ink/10 hover:text-text-primary"
            >
              <ArrowLeft aria-hidden="true" className="h-4 w-4 rtl:-scale-x-100" />
              {t('settingsLayout.allSettings')}
              {activeTitleKey && <span className="sr-only">{t('settingsLayout.leaving', { title: t(activeTitleKey) })}</span>}
            </button>
          )}
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
      <h2 id={titleId} className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
        {title}
      </h2>
      <div
        ref={cardRef}
        className={clsx(
          'divide-y divide-border rounded-xl ui-card bg-bg-card px-4',
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
  children: (ids: { labelId: string; hintId: string | undefined }) => React.ReactNode;
}> = ({ label, hint, keywords, stacked = false, children }) => {
  const labelId = useId();
  const hintId = useId();
  const { query } = useContext(ViewContext);
  const sectionMatches = useContext(SectionMatchContext);
  if (query && !sectionMatches && !settingsMatch(`${label} ${hint ?? ''} ${keywords ?? ''}`, query)) return null;
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
          {label}
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
