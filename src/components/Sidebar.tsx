import React from 'react';
import { NavLink } from 'react-router-dom';
import {
  Home,
  Compass,
  Bookmark,
  Calendar,
  Settings,
  ChevronLeft,
  ChevronRight,
  Search,
  HelpCircle,
  Bell,
  Download,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import clsx from 'clsx';
import { useUIStore } from '../stores/uiStore';
import { useMediaQuery } from '../hooks/useMediaQuery';
import { useUnreadNoticeCount } from '../hooks/useNotices';
import { useActiveDownloadCount } from '../hooks/useDownloads';
import { t, type TKey } from '../i18n';

const NAV_ITEMS: { to: string; icon: LucideIcon; labelKey: TKey }[] = [
  { to: '/', icon: Home, labelKey: 'sidebar.home' },
  { to: '/discover', icon: Compass, labelKey: 'sidebar.discover' },
  { to: '/schedule', icon: Calendar, labelKey: 'sidebar.schedule' },
  { to: '/library', icon: Bookmark, labelKey: 'sidebar.library' },
];

const itemClass = (expanded: boolean) =>
  clsx(
    'relative flex h-9 w-full items-center gap-3 rounded-md text-[13px] font-medium cursor-pointer transition-colors',
    expanded ? 'px-3' : 'justify-center'
  );
const IDLE_CLASS = 'text-text-secondary hover:bg-ink/5 hover:text-text-primary';

const SidebarLink: React.FC<{
  to: string;
  icon: LucideIcon;
  label: string;
  expanded: boolean;
  /** A count shown on the item (unread, in progress, …); hidden at 0. */
  badge?: number;
  /** The label with its count, for screen readers: "Notifications · 3 unread". */
  badgeLabel?: string;
}> = ({ to, icon: Icon, label, expanded, badge = 0, badgeLabel = '' }) => (
  <NavLink
    to={to}
    end={to === '/'}
    title={expanded ? undefined : badge > 0 ? `${label} (${badge})` : label}
    aria-label={badge > 0 && badgeLabel ? badgeLabel : label}
    draggable={false}
    className={({ isActive }) =>
      clsx(itemClass(expanded), isActive ? 'ui-nav-active bg-accent-tint text-accent-300' : IDLE_CLASS)
    }
  >
    {({ isActive }) => (
      <>
        {isActive && (
          <span
            aria-hidden="true"
            className="ui-nav-indicator absolute start-0 top-2 bottom-2 w-0.5 rounded-full bg-accent-500"
          />
        )}
        <Icon aria-hidden="true" className="h-5 w-5 flex-shrink-0" />
        {expanded && <span className="flex-1 truncate">{label}</span>}
        {badge > 0 && (
          <span
            aria-hidden="true"
            className={clsx(
              'min-w-[18px] rounded-full bg-accent-600 px-1 text-center text-[11px] font-semibold leading-[18px] text-on-accent tabular-nums',
              !expanded && 'absolute end-1 top-0.5'
            )}
          >
            {badge > 99 ? '99+' : badge}
          </span>
        )}
      </>
    )}
  </NavLink>
);

export const Sidebar: React.FC = () => {
  const sidebarExpanded = useUIStore((s) => s.sidebarExpanded);
  const toggleSidebar = useUIStore((s) => s.toggleSidebar);
  const setCommandPaletteOpen = useUIStore((s) => s.setCommandPaletteOpen);
  const setHotkeyCheatSheetOpen = useUIStore((s) => s.setHotkeyCheatSheetOpen);

  // Below 1100px the expanded sidebar leaves too little room for content.
  const isNarrow = useMediaQuery('(max-width: 1099px)');
  const expanded = sidebarExpanded && !isNarrow;
  const unreadNotices = useUnreadNoticeCount().data ?? 0;
  const queuedDownloads = useActiveDownloadCount();

  return (
    <aside
      className={clsx(
        'ui-sidebar relative z-30 flex h-full flex-shrink-0 flex-col justify-between border-e border-border bg-bg-deepest select-none transition-[width] duration-200',
        expanded ? 'w-56' : 'w-16'
      )}
    >
      <nav aria-label={t('sidebar.mainNav')} className="flex flex-col gap-1 p-2 pt-3">
        {NAV_ITEMS.map(({ labelKey, ...item }) => (
          <SidebarLink key={item.to} {...item} label={t(labelKey)} expanded={expanded} />
        ))}
        <SidebarLink
          to="/downloads"
          icon={Download}
          label={t('sidebar.downloads')}
          expanded={expanded}
          badge={queuedDownloads}
          badgeLabel={t('sidebar.downloadsQueued', { count: queuedDownloads })}
        />
        <SidebarLink
          to="/notifications"
          icon={Bell}
          label={t('sidebar.notifications')}
          expanded={expanded}
          badge={unreadNotices}
          badgeLabel={t('sidebar.notificationsUnread', { count: unreadNotices })}
        />
      </nav>

      <div className="flex flex-col gap-1 p-2">
        <button
          type="button"
          onClick={() => setCommandPaletteOpen(true)}
          title={expanded ? undefined : t('sidebar.searchHint')}
          aria-label={t('common.search')}
          className={clsx(itemClass(expanded), IDLE_CLASS)}
        >
          <Search aria-hidden="true" className="h-5 w-5 flex-shrink-0" />
          {expanded && (
            <>
              <span className="flex-1 truncate text-start">{t('common.search')}</span>
              <kbd className="rounded-sm bg-ink/10 px-1.5 py-0.5 text-[11px] font-medium text-text-muted">
                Ctrl+K
              </kbd>
            </>
          )}
        </button>

        <button
          type="button"
          onClick={() => setHotkeyCheatSheetOpen(true)}
          title={expanded ? undefined : t('sidebar.shortcutsHint')}
          aria-label={t('sidebar.keyboardShortcuts')}
          className={clsx(itemClass(expanded), IDLE_CLASS)}
        >
          <HelpCircle aria-hidden="true" className="h-5 w-5 flex-shrink-0" />
          {expanded && (
            <>
              <span className="flex-1 truncate text-start">{t('sidebar.shortcuts')}</span>
              <kbd className="rounded-sm bg-ink/10 px-1.5 py-0.5 text-[11px] font-medium text-text-muted">
                ?
              </kbd>
            </>
          )}
        </button>

        <SidebarLink to="/settings" icon={Settings} label={t('common.settings')} expanded={expanded} />

        {!isNarrow && (
          <button
            type="button"
            onClick={toggleSidebar}
            aria-label={expanded ? t('sidebar.collapseSidebar') : t('sidebar.expandSidebar')}
            aria-expanded={expanded}
            title={expanded ? undefined : t('sidebar.expandSidebar')}
            className={clsx(itemClass(expanded), 'text-text-muted hover:bg-ink/5 hover:text-text-primary')}
          >
            {expanded ? (
              <>
                <ChevronLeft aria-hidden="true" className="h-5 w-5 flex-shrink-0 rtl:-scale-x-100" />
                <span className="truncate">{t('sidebar.collapse')}</span>
              </>
            ) : (
              <ChevronRight aria-hidden="true" className="h-5 w-5 flex-shrink-0 rtl:-scale-x-100" />
            )}
          </button>
        )}
      </div>
    </aside>
  );
};
