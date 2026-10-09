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
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import clsx from 'clsx';
import { useUIStore } from '../stores/uiStore';
import { useMediaQuery } from '../hooks/useMediaQuery';

const NAV_ITEMS: { to: string; icon: LucideIcon; label: string }[] = [
  { to: '/', icon: Home, label: 'Home' },
  { to: '/discover', icon: Compass, label: 'Discover' },
  { to: '/schedule', icon: Calendar, label: 'Schedule' },
  { to: '/library', icon: Bookmark, label: 'Library' },
];

const itemClass = (expanded: boolean) =>
  clsx(
    'relative flex h-9 w-full items-center gap-3 rounded-md text-[13px] font-medium cursor-pointer transition-colors',
    expanded ? 'px-3' : 'justify-center'
  );
const IDLE_CLASS = 'text-text-secondary hover:bg-white/5 hover:text-text-primary';

const SidebarLink: React.FC<{
  to: string;
  icon: LucideIcon;
  label: string;
  expanded: boolean;
}> = ({ to, icon: Icon, label, expanded }) => (
  <NavLink
    to={to}
    end={to === '/'}
    title={expanded ? undefined : label}
    aria-label={label}
    draggable={false}
    className={({ isActive }) =>
      clsx(itemClass(expanded), isActive ? 'bg-accent-tint text-accent-300' : IDLE_CLASS)
    }
  >
    {({ isActive }) => (
      <>
        {isActive && (
          <span
            aria-hidden="true"
            className="absolute left-0 top-2 bottom-2 w-0.5 rounded-full bg-accent-500"
          />
        )}
        <Icon aria-hidden="true" className="h-5 w-5 flex-shrink-0" />
        {expanded && <span className="truncate">{label}</span>}
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

  return (
    <aside
      className={clsx(
        'relative z-30 flex h-full flex-shrink-0 flex-col justify-between border-r border-border bg-bg-deepest select-none transition-[width] duration-200',
        expanded ? 'w-56' : 'w-16'
      )}
    >
      <nav aria-label="Main" className="flex flex-col gap-1 p-2 pt-3">
        {NAV_ITEMS.map((item) => (
          <SidebarLink key={item.to} {...item} expanded={expanded} />
        ))}
      </nav>

      <div className="flex flex-col gap-1 p-2">
        <button
          type="button"
          onClick={() => setCommandPaletteOpen(true)}
          title={expanded ? undefined : 'Search (Ctrl+K)'}
          aria-label="Search"
          className={clsx(itemClass(expanded), IDLE_CLASS)}
        >
          <Search aria-hidden="true" className="h-5 w-5 flex-shrink-0" />
          {expanded && (
            <>
              <span className="flex-1 truncate text-left">Search</span>
              <kbd className="rounded-sm bg-white/10 px-1.5 py-0.5 text-[11px] font-medium text-text-muted">
                Ctrl+K
              </kbd>
            </>
          )}
        </button>

        <button
          type="button"
          onClick={() => setHotkeyCheatSheetOpen(true)}
          title={expanded ? undefined : 'Keyboard shortcuts (?)'}
          aria-label="Keyboard shortcuts"
          className={clsx(itemClass(expanded), IDLE_CLASS)}
        >
          <HelpCircle aria-hidden="true" className="h-5 w-5 flex-shrink-0" />
          {expanded && (
            <>
              <span className="flex-1 truncate text-left">Shortcuts</span>
              <kbd className="rounded-sm bg-white/10 px-1.5 py-0.5 text-[11px] font-medium text-text-muted">
                ?
              </kbd>
            </>
          )}
        </button>

        <SidebarLink to="/settings" icon={Settings} label="Settings" expanded={expanded} />

        {!isNarrow && (
          <button
            type="button"
            onClick={toggleSidebar}
            aria-label={expanded ? 'Collapse sidebar' : 'Expand sidebar'}
            aria-expanded={expanded}
            title={expanded ? undefined : 'Expand sidebar'}
            className={clsx(itemClass(expanded), 'text-text-muted hover:bg-white/5 hover:text-text-primary')}
          >
            {expanded ? (
              <>
                <ChevronLeft aria-hidden="true" className="h-5 w-5 flex-shrink-0" />
                <span className="truncate">Collapse</span>
              </>
            ) : (
              <ChevronRight aria-hidden="true" className="h-5 w-5 flex-shrink-0" />
            )}
          </button>
        )}
      </div>
    </aside>
  );
};
