import { useCallback, useEffect, useState } from 'react';

// Older WebKitGTK builds only expose the prefixed Fullscreen API.
interface PrefixedDocument extends Document {
  webkitFullscreenElement?: Element | null;
  webkitExitFullscreen?: () => Promise<void> | void;
}

interface PrefixedElement extends HTMLElement {
  webkitRequestFullscreen?: () => Promise<void> | void;
}

const CHANGE_EVENTS = ['fullscreenchange', 'webkitfullscreenchange'] as const;

export function getFullscreenElement(): Element | null {
  const doc = document as PrefixedDocument;
  return doc.fullscreenElement ?? doc.webkitFullscreenElement ?? null;
}

export function exitFullscreen(): void {
  if (!getFullscreenElement()) return;
  const doc = document as PrefixedDocument;
  try {
    const result = doc.exitFullscreen ? doc.exitFullscreen() : doc.webkitExitFullscreen?.();
    if (result instanceof Promise) result.catch(() => {});
  } catch {
    // Already left fullscreen.
  }
}

function requestFullscreen(element: HTMLElement): void {
  const target = element as PrefixedElement;
  try {
    const result = target.requestFullscreen
      ? target.requestFullscreen()
      : target.webkitRequestFullscreen?.();
    if (result instanceof Promise) result.catch(() => {});
  } catch {
    // The webview refused; the state stays in sync through the change event.
  }
}

/** The element currently shown fullscreen, kept in sync with the browser. */
export function useFullscreenElement(): HTMLElement | null {
  const [element, setElement] = useState<HTMLElement | null>(null);

  useEffect(() => {
    const sync = () => {
      const current = getFullscreenElement();
      setElement(current instanceof HTMLElement ? current : null);
    };
    sync();
    CHANGE_EVENTS.forEach((name) => document.addEventListener(name, sync));
    return () => CHANGE_EVENTS.forEach((name) => document.removeEventListener(name, sync));
  }, []);

  return element;
}

/** Fullscreen state of `target`, read from the browser rather than assumed. */
export function useFullscreen(target: HTMLElement | null) {
  const fullscreenElement = useFullscreenElement();
  const isFullscreen = target !== null && fullscreenElement === target;

  const toggle = useCallback(() => {
    if (getFullscreenElement()) exitFullscreen();
    else if (target) requestFullscreen(target);
  }, [target]);

  return { isFullscreen, toggle };
}
