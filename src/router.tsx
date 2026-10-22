import React, { useEffect, useLayoutEffect, useRef } from 'react';
import {
  createHashRouter,
  Navigate,
  Outlet,
  useLocation,
  useNavigate,
  useNavigationType,
  useParams,
} from 'react-router-dom';
import { onOpenUrl, getCurrent } from '@tauri-apps/plugin-deep-link';
import { listen } from '@tauri-apps/api/event';
import { TitleBar } from './components/TitleBar';
import { OfflineBanner } from './components/OfflineBanner';
import { ErrorBoundary } from './components/ErrorBoundary';
import { Sidebar } from './components/Sidebar';
import { ToastContainer } from './components/Toast';
import { AuthDialog } from './components/AuthDialog';
import { BulkDownloadDialog } from './components/BulkDownloadDialog';
import { UpdateDialog } from './components/UpdateDialog';
import { EngineConfirmDialog } from './components/player/EngineConfirmDialog';
import { CommandPalette } from './components/CommandPalette';
import { HotkeyCheatSheet } from './components/HotkeyCheatSheet';
import { Button } from './components/ui/Button';
import { EmptyState } from './components/ui/EmptyState';
import { hasOpenModal } from './components/ui/Modal';
import { usePluginEvents } from './hooks/usePlugins';
import { usePlayful } from './hooks/useSettings';
import { useUIStore } from './stores/uiStore';
import { t } from './i18n';
import { Home } from './screens/Home';
import { Discover } from './screens/Discover';
import { Detail } from './screens/Detail';
import { Watch } from './screens/Watch';
import { Library } from './screens/Library';
import { Schedule } from './screens/Schedule';
import { Settings } from './screens/Settings';
import { Notifications } from './screens/Notifications';
import { Downloads } from './screens/Downloads';

/** `anix://anime/<id>` (or the older `anilili://…`) → the id, or null for anything else. Negative ids are catalogue stand-ins. */
function parseDeepLink(url: string): string | null {
  const match = url.trim().match(/^(?:anix|anilili):\/\/anime\/(-?[1-9]\d*)\/?(?:[?#].*)?$/i);
  return match ? match[1] : null;
}

// The URL the app was launched with is delivered once; later mounts must not replay it.
let launchUrlHandled = false;

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.tagName === 'SELECT' ||
    target.isContentEditable
  );
}

const AppLayout: React.FC = () => {
  const location = useLocation();
  const navigate = useNavigate();
  const navigationType = useNavigationType();
  const isWatchScreen = location.pathname.startsWith('/watch/');

  // The layout lives as long as the app, so plugin state stays current on every screen.
  usePluginEvents();

  const commandPaletteOpen = useUIStore((s) => s.commandPaletteOpen);
  const playful = usePlayful();
  const setCommandPaletteOpen = useUIStore((s) => s.setCommandPaletteOpen);
  const hotkeyCheatSheetOpen = useUIStore((s) => s.hotkeyCheatSheetOpen);
  const setHotkeyCheatSheetOpen = useUIStore((s) => s.setHotkeyCheatSheetOpen);

  // Deep links: anix://anime/:id (and the older anilili://), at launch and while running (the single-instance
  // plugin forwards a second launch to this listener).
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const handleUrls = (urls: string[] | null | undefined) => {
      for (const url of urls ?? []) {
        const id = parseDeepLink(url);
        if (id) {
          navigate(`/detail/${id}`);
          return;
        }
        console.warn('[DeepLink] ignored unsupported URL:', url);
      }
    };

    if (!launchUrlHandled) {
      launchUrlHandled = true;
      getCurrent()
        .then((urls) => {
          if (!cancelled) handleUrls(urls);
        })
        .catch((err) => {
          console.debug('[DeepLink] getCurrent failed (not running in Tauri?):', err);
        });
    }

    onOpenUrl(handleUrls)
      .then((fn) => {
        // The effect may have been cleaned up before the listener was registered.
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch((err) => {
        console.debug('[DeepLink] onOpenUrl failed (not running in Tauri?):', err);
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [navigate]);

  // A clicked release notification: the backend sends the AniList id of the anime to open.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    listen<number>('open-anime', (event) => {
      const id = Number(event.payload);
      if (Number.isInteger(id) && id > 0) navigate(`/detail/${id}`);
    })
      .then((fn) => {
        // The effect may have been cleaned up before the listener was registered.
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch((err) => {
        console.debug('[Notifications] listen failed (not running in Tauri?):', err);
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [navigate]);

  // Global hotkeys: Ctrl/Cmd+K (palette, works while typing) and ? (shortcut list).
  useEffect(() => {
    const handleGlobalKeyDown = (e: KeyboardEvent) => {
      if (e.repeat || e.isComposing) return;
      const ui = useUIStore.getState();

      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        // Another dialog is in front: do not stack the palette on top of it.
        if (!ui.commandPaletteOpen && hasOpenModal()) return;
        ui.setCommandPaletteOpen(!ui.commandPaletteOpen);
        return;
      }

      if (e.key === '?' && !e.ctrlKey && !e.metaKey && !e.altKey) {
        if (isEditableTarget(e.target)) return;
        if (ui.hotkeyCheatSheetOpen) {
          e.preventDefault();
          ui.setHotkeyCheatSheetOpen(false);
        } else if (!hasOpenModal()) {
          e.preventDefault();
          ui.setHotkeyCheatSheetOpen(true);
        }
      }
    };

    window.addEventListener('keydown', handleGlobalKeyDown);
    return () => window.removeEventListener('keydown', handleGlobalKeyDown);
  }, []);

  // Scroll: a new screen starts at the top; Back/Forward returns to where it was.
  const mainRef = useRef<HTMLElement>(null);
  const scrollPositions = useRef(new Map<string, number>());
  const locationKeyRef = useRef(location.key);

  useLayoutEffect(() => {
    const main = mainRef.current;
    locationKeyRef.current = location.key;
    if (!main) return;
    if (navigationType === 'REPLACE') {
      scrollPositions.current.set(location.key, main.scrollTop);
    } else {
      main.scrollTop = navigationType === 'POP' ? scrollPositions.current.get(location.key) ?? 0 : 0;
    }
    // The clicked link is often gone with the old screen: start keyboard and screen-reader users
    // at the new screen's heading instead of the top of the window.
    const active = document.activeElement;
    if (navigationType === 'PUSH' && (!active || active === document.body || !active.isConnected)) {
      const target = main.querySelector<HTMLElement>('h1') ?? main;
      if (!target.hasAttribute('tabindex')) target.tabIndex = -1;
      target.focus({ preventScroll: true });
    }
  }, [location.key, navigationType]);

  return (
    <div className="ui-app flex h-screen w-screen flex-col overflow-hidden bg-bg-page text-text-primary select-none">
      <TitleBar />
      <OfflineBanner />

      <div className="flex min-h-0 w-full flex-1 overflow-hidden">
        {!isWatchScreen && <Sidebar />}

        <main
          ref={mainRef}
          onScroll={(e) => scrollPositions.current.set(locationKeyRef.current, e.currentTarget.scrollTop)}
          // The 6px scrollbar gutter is always reserved, so content keeps its width whether or
          // not a screen is long enough to scroll; the right padding is 6px less to balance it.
          className={`h-full min-w-0 flex-1 ${
            isWatchScreen
              ? 'overflow-hidden'
              : 'overflow-y-scroll py-6 ps-6 pe-[18px] xl:py-8 xl:ps-8 xl:pe-[26px]'
          }`}
        >
          <ErrorBoundary resetKey={location.pathname} playful={playful}>
            {isWatchScreen ? (
              <Outlet />
            ) : (
              <div className="mx-auto w-full max-w-[1600px]">
                <Outlet />
              </div>
            )}
          </ErrorBoundary>
        </main>
      </div>

      <ToastContainer placement={isWatchScreen ? 'top-center' : 'bottom-right'} />
      <AuthDialog />
      <BulkDownloadDialog />
      <UpdateDialog />
      <EngineConfirmDialog />
      <CommandPalette
        isOpen={commandPaletteOpen}
        onClose={() => setCommandPaletteOpen(false)}
      />
      <HotkeyCheatSheet
        isOpen={hotkeyCheatSheetOpen}
        onClose={() => setHotkeyCheatSheetOpen(false)}
      />
    </div>
  );
};

/** Rejects `/watch/abc/x` and the like before the player sees NaN ids. */
const WatchRoute: React.FC = () => {
  const { id, episodeNum } = useParams<{ id: string; episodeNum: string }>();
  const navigate = useNavigate();
  const idValid = !!id && /^-?\d+$/.test(id);
  const episodeValid = !!episodeNum && /^\d+(\.\d+)?$/.test(episodeNum);

  if (!idValid || !episodeValid) {
    return (
      <EmptyState
        className="h-full"
        title={t('router.invalidEpisode')}
        description={t('router.invalidEpisodeDetail', { path: `/watch/${id ?? ''}/${episodeNum ?? ''}` })}
        action={
          <Button
            variant="secondary"
            onClick={() => navigate(idValid ? `/detail/${id}` : '/', { replace: true })}
          >
            {idValid ? t('router.openAnime') : t('router.goHome')}
          </Button>
        }
      />
    );
  }

  return <Watch />;
};

export const router = createHashRouter([
  {
    path: '/',
    element: <AppLayout />,
    children: [
      { index: true, element: <Home /> },
      { path: 'discover', element: <Discover /> },
      { path: 'detail/:id', element: <Detail /> },
      { path: 'watch/:id/:episodeNum', element: <WatchRoute /> },
      { path: 'library', element: <Library /> },
      { path: 'schedule', element: <Schedule /> },
      { path: 'settings', element: <Settings /> },
      { path: 'notifications', element: <Notifications /> },
      { path: 'downloads', element: <Downloads /> },
      { path: '*', element: <Navigate to="/" replace /> },
    ],
  },
]);
