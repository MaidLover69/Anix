import { useCallback, useEffect } from 'react';
import { currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window';
import { usePlayerStore } from '../stores/playerStore';
import { exitFullscreen } from './useFullscreen';

/** The window's normal minimum size (tauri.conf.json). */
const NORMAL_MIN = new LogicalSize(960, 600);
const MINI_MIN = new LogicalSize(320, 180);
const MINI_SIZE = new LogicalSize(480, 270);
const MINI_MARGIN = 24;

interface SavedGeometry {
  maximized: boolean;
  size: PhysicalSize | null;
  position: PhysicalPosition | null;
}

// The window outlives the per-episode player, so its normal geometry is kept here.
let saved: SavedGeometry | null = null;
// Window changes run one after another, so a quick toggle back cannot interleave with them.
let pending: Promise<void> = Promise.resolve();

async function attempt<T>(step: () => Promise<T>): Promise<T | null> {
  try {
    return await step();
  } catch {
    // Some steps are not available everywhere (Wayland cannot place or raise windows).
    return null;
  }
}

async function enterMini(): Promise<void> {
  const win = getCurrentWindow();
  exitFullscreen();
  saved = {
    maximized: (await attempt(() => win.isMaximized())) ?? false,
    size: await attempt(() => win.innerSize()),
    position: await attempt(() => win.outerPosition()),
  };
  if (saved.maximized) await attempt(() => win.unmaximize());
  await attempt(() => win.setMinSize(MINI_MIN));
  await attempt(() => win.setSize(MINI_SIZE));
  await attempt(() => win.setAlwaysOnTop(true));
  // Bottom right of the current screen.
  const monitor = await attempt(() => currentMonitor());
  if (monitor) {
    const scale = monitor.scaleFactor || 1;
    const width = MINI_SIZE.width * scale;
    const height = MINI_SIZE.height * scale;
    const margin = MINI_MARGIN * scale;
    const x = monitor.position.x + monitor.size.width - width - margin;
    const y = monitor.position.y + monitor.size.height - height - margin;
    await attempt(() => win.setPosition(new PhysicalPosition(Math.round(x), Math.round(y))));
  }
}

async function exitMini(): Promise<void> {
  const win = getCurrentWindow();
  const geometry = saved;
  saved = null;
  await attempt(() => win.setAlwaysOnTop(false));
  await attempt(() => win.setMinSize(NORMAL_MIN));
  if (geometry?.size) await attempt(() => win.setSize(geometry.size!));
  else await attempt(() => win.setSize(NORMAL_MIN));
  if (geometry?.position) await attempt(() => win.setPosition(geometry.position!));
  if (geometry?.maximized) await attempt(() => win.maximize());
}

/** Switches the main window between its normal layout and the always-on-top mini-player. */
export function setMiniPlayer(mini: boolean): void {
  const store = usePlayerStore.getState();
  if (store.miniPlayer === mini) return;
  store.setMiniPlayer(mini);
  pending = pending.then(mini ? enterMini : exitMini, mini ? enterMini : exitMini);
}

export function useMiniPlayer() {
  const mini = usePlayerStore((s) => s.miniPlayer);
  const toggle = useCallback(() => setMiniPlayer(!usePlayerStore.getState().miniPlayer), []);
  return { mini, toggle, exit: useCallback(() => setMiniPlayer(false), []) };
}

/** For the Watch screen: leaving it always gives the window its normal size back. */
export function useMiniPlayerSession(): boolean {
  const mini = usePlayerStore((s) => s.miniPlayer);
  useEffect(() => () => setMiniPlayer(false), []);
  return mini;
}

/** True when the webview offers the browser's own picture-in-picture. */
export function supportsPictureInPicture(video: HTMLVideoElement | null): boolean {
  return (
    !!video &&
    typeof document !== 'undefined' &&
    'pictureInPictureEnabled' in document &&
    document.pictureInPictureEnabled === true &&
    typeof video.requestPictureInPicture === 'function'
  );
}
