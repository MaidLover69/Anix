/**
 * Player fullscreen without the browser's Fullscreen API: the window goes fullscreen and the
 * player element covers it (`.app-fullscreen`, `html.player-fullscreen` in global.css), with
 * only the player painted, as in browser fullscreen. WebKitGTK 2.50 (Ubuntu 22.04/24.04 and
 * the WebKit the AppImage bundles) aborts the whole app when its own fullscreen manager is torn
 * down, so `requestFullscreen` is never called.
 */
import { useCallback, useEffect, useSyncExternalStore } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

const ELEMENT_CLASS = 'app-fullscreen';
const ROOT_CLASS = 'player-fullscreen';

let current: HTMLElement | null = null;
/** The desktop showed the window fullscreen since the request, so a later normal window means the user left it. */
let confirmed = false;
/** The window was already fullscreen (the user's own) when the player went fullscreen: leave it so. */
let windowWasFullscreen = false;
let watching = false;
const listeners = new Set<() => void>();

/** The desktop's view, not Tauri's `isFullscreen`, which only repeats the last request on Linux. */
const windowIsFullscreen = () => invoke<boolean>('window_is_fullscreen').catch(() => true); // i18n-ignore

function onKeyDown(e: KeyboardEvent): void {
  // Bubble phase: dialogs, drawers and menus handle their own Escape first and mark it handled.
  if (e.key === 'Escape' && !e.defaultPrevented) void exitFullscreen();
}

function setCurrent(element: HTMLElement | null): void {
  if (element === current) return;
  current?.classList.remove(ELEMENT_CLASS);
  current = element;
  confirmed = false;
  element?.classList.add(ELEMENT_CLASS);
  document.documentElement.classList.toggle(ROOT_CLASS, element !== null);
  if (element) window.addEventListener('keydown', onKeyDown);
  else window.removeEventListener('keydown', onKeyDown);
  listeners.forEach((listener) => listener());
}

/** Leaving fullscreen through the desktop (a window-manager key) ends the player's fullscreen too. */
function watchWindow(): void {
  if (watching) return;
  watching = true;
  getCurrentWindow()
    .onResized(async () => {
      if (!current) return;
      const fullscreen = await windowIsFullscreen();
      if (!current) return;
      if (fullscreen) confirmed = true;
      else if (confirmed) setCurrent(null);
    })
    .catch(() => {
      watching = false;
    });
}

export function getFullscreenElement(): HTMLElement | null {
  return current;
}

/** Leaves the player's fullscreen; resolves once the window has been asked to leave it too. */
export async function exitFullscreen(): Promise<void> {
  if (!current) return;
  setCurrent(null);
  if (windowWasFullscreen) return;
  await getCurrentWindow()
    .setFullscreen(false)
    .catch(() => {});
}

async function requestFullscreen(element: HTMLElement): Promise<void> {
  watchWindow();
  setCurrent(element);
  windowWasFullscreen = await windowIsFullscreen().then((v) => v, () => false);
  if (current !== element) return;
  if (windowWasFullscreen) {
    confirmed = true;
    return;
  }
  await getCurrentWindow()
    .setFullscreen(true)
    .catch(() => {
      if (current === element) setCurrent(null);
    });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The element currently shown fullscreen. */
export function useFullscreenElement(): HTMLElement | null {
  return useSyncExternalStore(subscribe, getFullscreenElement);
}

/** Fullscreen state of `target`; fullscreen ends when `target` goes away. */
export function useFullscreen(target: HTMLElement | null) {
  const fullscreenElement = useFullscreenElement();
  const isFullscreen = target !== null && fullscreenElement === target;

  useEffect(
    () => () => {
      if (target && current === target) void exitFullscreen();
    },
    [target]
  );

  const toggle = useCallback(() => {
    if (current) void exitFullscreen();
    else if (target) void requestFullscreen(target);
  }, [target]);

  return { isFullscreen, toggle };
}
