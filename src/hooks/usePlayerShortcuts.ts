import { useEffect, useRef } from 'react';
import { hasOpenModal } from '../components/ui/Modal';
import { useUIStore } from '../stores/uiStore';

export interface PlayerShortcutActions {
  togglePlay: () => void;
  seekBy: (seconds: number) => void;
  volumeBy: (delta: number) => void;
  toggleMute: () => void;
  toggleFullscreen: () => void;
  exitFullscreen: () => void;
  nextEpisode: () => void;
  previousEpisode: () => void;
  skipSegment: () => void;
}

const TEXT_INPUT_TYPES = new Set(['text', 'search', 'email', 'password', 'number', 'tel', 'url']);

function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  return target instanceof HTMLInputElement && TEXT_INPUT_TYPES.has(target.type);
}

/** A focused control keeps Space and Enter for itself. */
function isActivatable(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    target.closest('button, a[href], [role="slider"], [role="menu"], [role="switch"], [role="tab"]') !== null
  );
}

function anyDialogOpen(): boolean {
  const ui = useUIStore.getState();
  return (
    hasOpenModal() ||
    ui.commandPaletteOpen ||
    ui.hotkeyCheatSheetOpen ||
    ui.authDialogOpen ||
    ui.updateDialogOpen ||
    ui.bulkDownloadDialogOpen ||
    document.querySelector('[aria-modal="true"]') !== null
  );
}

/**
 * Keyboard shortcuts of the player. They stand down while a modifier other than Shift is
 * held, while focus is in a text field and while any dialog or drawer is open.
 * `HotkeyCheatSheet` lists exactly the keys handled here.
 */
export function usePlayerShortcuts(actions: PlayerShortcutActions) {
  const actionsRef = useRef(actions);
  actionsRef.current = actions;

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
      if (isTextEntry(e.target) || anyDialogOpen()) return;

      const a = actionsRef.current;
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      let handled = true;

      switch (key) {
        case ' ':
        case 'k':
          if (e.shiftKey || (key === ' ' && isActivatable(e.target))) handled = false;
          else if (!e.repeat) a.togglePlay();
          break;
        case 'f':
          if (e.shiftKey) handled = false;
          else if (!e.repeat) a.toggleFullscreen();
          break;
        case 'm':
          if (e.shiftKey) handled = false;
          else if (!e.repeat) a.toggleMute();
          break;
        case 's':
          if (e.shiftKey) handled = false;
          else if (!e.repeat) a.skipSegment();
          break;
        case 'j':
          a.seekBy(-10);
          break;
        case 'l':
          a.seekBy(10);
          break;
        case 'ArrowLeft':
          a.seekBy(e.shiftKey ? -30 : -5);
          break;
        case 'ArrowRight':
          a.seekBy(e.shiftKey ? 30 : 5);
          break;
        case 'ArrowUp':
          a.volumeBy(0.05);
          break;
        case 'ArrowDown':
          a.volumeBy(-0.05);
          break;
        case 'n':
          if (!e.shiftKey) handled = false;
          else if (!e.repeat) a.nextEpisode();
          break;
        case 'p':
          if (!e.shiftKey) handled = false;
          else if (!e.repeat) a.previousEpisode();
          break;
        case 'Escape':
          // Only ever leaves fullscreen; the webview usually handles this key itself.
          a.exitFullscreen();
          handled = false;
          break;
        default:
          handled = false;
      }

      if (handled) e.preventDefault();
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}
