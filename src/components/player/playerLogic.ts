// Small pure rules of the player, kept apart so they can be checked without a webview.

import { normalizeLanguage } from '../../lib/languages';

/**
 * Index of the first audio track in a wanted language, trying the languages in order. Used by
 * both engines; a dub stream that marks Japanese as its default still starts in the dub's
 * language this way.
 */
export function preferredAudioTrack(tracks: { lang?: string | null; name?: string | null }[], wanted: string[]): number {
  const codes = tracks.map((track) => normalizeLanguage(track.lang) ?? normalizeLanguage(track.name));
  for (const language of wanted) {
    const index = codes.indexOf(language.toLowerCase());
    if (index !== -1) return index;
  }
  return -1;
}

/** `[` and `]` change the speed in these steps, within these bounds. */
export const SPEED_STEP = 0.25;
export const MIN_SPEED = 0.25;
export const MAX_SPEED = 3;

export function stepSpeed(rate: number, direction: 1 | -1): number {
  const current = Number.isFinite(rate) && rate > 0 ? rate : 1;
  // Snap to the grid first, so 1.1 goes to 1.25 / 1, not 1.35 / 0.85.
  const snapped = direction > 0 ? Math.floor(current / SPEED_STEP + 1e-9) : Math.ceil(current / SPEED_STEP - 1e-9);
  const next = (snapped + direction) * SPEED_STEP;
  return Math.min(MAX_SPEED, Math.max(MIN_SPEED, next));
}

/** Holding Space longer than this plays at `HOLD_RATE` until it is let go; a tap toggles play. */
export const HOLD_MS = 300;
export const HOLD_RATE = 2;

export function isHold(pressedAt: number, now: number): boolean {
  return now - pressedAt >= HOLD_MS;
}

/** Auto-skip counts down on the button before it jumps. */
export const SKIP_COUNTDOWN_SECONDS = 3;

/** Seconds left of a countdown started at `startedAt` (ms), 0 when it is time to skip. */
export function countdownLeft(startedAt: number, now: number): number {
  return Math.max(0, SKIP_COUNTDOWN_SECONDS - (now - startedAt) / 1000);
}

/** The number shown on the button: 3, 2, 1. */
export function countdownLabel(secondsLeft: number): number {
  return Math.max(1, Math.ceil(secondsLeft - 1e-9));
}

/** Without known skip times, a fixed jump is offered at the start of an episode. */
export const FALLBACK_SKIP_SECONDS = 85;
const FALLBACK_SKIP_UNTIL = 180;
const FALLBACK_MIN_DURATION = 600;

/** Where "Skip +85 s" jumps to, or null when it is not offered. */
export function fallbackSkipTarget(time: number, duration: number, hasSkipTimes: boolean): number | null {
  if (hasSkipTimes || !(duration >= FALLBACK_MIN_DURATION) || !(time >= 0) || time >= FALLBACK_SKIP_UNTIL) return null;
  return time + FALLBACK_SKIP_SECONDS;
}

/** After this long paused, the title and episode are shown over the picture. */
export const PAUSE_DETAILS_MS = 5000;

/** "Ends at": the wall-clock time the episode ends at the current speed. */
export function endsAt(now: number, time: number, duration: number, rate: number): number | null {
  if (!(duration > 0) || !(time >= 0) || time > duration) return null;
  return now + ((duration - time) / (rate > 0 ? rate : 1)) * 1000;
}

export interface SubtitlePositionInput {
  /** Height of the video region, CSS px. */
  regionHeight: number;
  /** Heights of the control bar and the top bar while they show, CSS px. */
  bottomBar: number;
  topBar: number;
  /** The "Subtitle height" setting, percent of the picture. */
  raisePercent: number;
  chromeVisible: boolean;
}

/**
 * mpv's `sub-pos` (100 = bottom) for the subtitles and `secondary-sub-pos` for the second
 * one (0 = top): raised by the user's setting, and clear of the bars while they show.
 */
export function mpvSubtitlePositions(input: SubtitlePositionInput): { primary: number; secondary: number } {
  const raise = Math.min(40, Math.max(0, input.raisePercent || 0));
  if (!(input.regionHeight > 0)) return { primary: 100 - raise, secondary: 0 };
  const bottom = input.chromeVisible ? (Math.max(0, input.bottomBar) / input.regionHeight) * 100 : 0;
  const top = input.chromeVisible ? (Math.max(0, input.topBar) / input.regionHeight) * 100 : 0;
  return {
    primary: Math.round(Math.min(100, Math.max(0, 100 - Math.max(raise, bottom)))),
    secondary: Math.round(Math.min(50, top)),
  };
}

export type ScreenFit = 'fit' | 'crop' | 'fill';
export const SCREEN_FITS: readonly ScreenFit[] = ['fit', 'crop', 'fill'];

/** The web engine's `object-fit` class for a fit mode. */
export function objectFitClass(fit: ScreenFit): string {
  return fit === 'crop' ? 'object-cover' : fit === 'fill' ? 'object-fill' : 'object-contain';
}
