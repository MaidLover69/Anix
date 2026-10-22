import { useEffect, useRef } from 'react';
import { hasOpenModal } from '../components/ui/Modal';
import { useUIStore } from '../stores/uiStore';
import { HOLD_MS } from '../components/player/playerLogic';

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
  toggleMiniPlayer: () => void;
  /** `[` / `]`: one speed step down or up. */
  speedBy: (direction: 1 | -1) => void;
  /** Holding Space: play fast until it is let go. */
  holdStart: () => void;
  holdEnd: () => void;
  /** Enter / Esc while the skip countdown runs; true when there was one. */
  confirmSkip: () => boolean;
  cancelSkip: () => boolean;
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
    // Space is pressed since, its hold timer, and whether it plays fast right now.
    let spaceDownAt: number | null = null;
    let holdTimer: number | null = null;
    let holding = false;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
      if (isTextEntry(e.target) || anyDialogOpen()) return;

      const a = actionsRef.current;
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      let handled = true;

      switch (key) {
        case ' ':
          // A tap toggles play when it is let go; held longer, it plays fast (see keyup).
          if (e.shiftKey || isActivatable(e.target)) handled = false;
          else if (!e.repeat && spaceDownAt === null) {
            spaceDownAt = Date.now();
            holdTimer = window.setTimeout(() => {
              holding = true;
              actionsRef.current.holdStart();
            }, HOLD_MS);
          }
          break;
        case 'k':
          if (e.shiftKey) handled = false;
          else if (!e.repeat) a.togglePlay();
          break;
        case '[':
          a.speedBy(-1);
          break;
        case ']':
          a.speedBy(1);
          break;
        case 'Enter':
          handled = !isActivatable(e.target) && a.confirmSkip();
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
        case 'i':
          if (e.shiftKey) handled = false;
          else if (!e.repeat) a.toggleMiniPlayer();
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
          // Cancels a skip countdown, else only ever leaves fullscreen; the webview usually
          // handles this key itself.
          if (a.cancelSkip()) break;
          a.exitFullscreen();
          handled = false;
          break;
        default:
          handled = false;
      }

      if (handled) e.preventDefault();
    };

    const endSpace = (toggle: boolean) => {
      if (holdTimer !== null) window.clearTimeout(holdTimer);
      holdTimer = null;
      const wasHolding = holding;
      const pressed = spaceDownAt !== null;
      spaceDownAt = null;
      holding = false;
      if (wasHolding) actionsRef.current.holdEnd();
      else if (pressed && toggle) actionsRef.current.togglePlay();
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key !== ' ' || spaceDownAt === null) return;
      e.preventDefault();
      endSpace(true);
    };
    // Focus leaving the window while Space is held must not leave the speed up.
    const onBlur = () => endSpace(false);

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onBlur);
    return () => {
      endSpace(false);
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onBlur);
    };
  }, []);
}
