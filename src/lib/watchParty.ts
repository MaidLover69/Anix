/**
 * Watch party maths for the player page: where the host is right now, whether a guest's
 * player has drifted far enough to correct, and whether the guest's episode is a different
 * cut. The backend turns the host's clock into ages, so only this page's clock is used here.
 */

/** Drift a guest tolerates before it corrects. */
export const DEAD_BAND_MS = 250;
/** After a correction the player gets this long to settle before the next one. */
export const QUIET_AFTER_ALIGN_MS = 2000;
/** A correction seeks this far ahead and starts when the host gets there. */
export const ALIGN_LEAD_MS = 1000;
/** Lengths further apart than this mean the guest has another version of the episode. */
export const DURATION_MISMATCH_MS = 2000;
/** A guest counts as ready once this much is buffered ahead of its position (or up to the end). */
export const READY_AHEAD_S = 2;

/**
 * True when `ranges` (the media's buffered ranges, in s) hold the next `READY_AHEAD_S` from `at`.
 * A paused web player can sit below `canplay` for good, so readiness goes by what is buffered.
 */
export function isBufferedAhead(ranges: ReadonlyArray<readonly [number, number]>, at: number, duration: number): boolean {
  const want = Number.isFinite(duration) && duration > 0 ? Math.min(at + READY_AHEAD_S, duration) : at + READY_AHEAD_S;
  return ranges.some(([start, end]) => start <= at + 0.05 && end >= want - 0.05);
}

/** The manual offset a guest can set, and its step. */
export const MAX_OFFSET_MS = 30_000;
export const OFFSET_STEP_MS = 500;

/** One report of the host, as the page received it. */
export interface HostSample {
  paused: boolean;
  positionMs: number;
  rate: number;
  /** How old the report was when it arrived (negative: its moment was still ahead). */
  ageMs: number;
  /** `performance.now()` when it arrived. */
  receivedAt: number;
}

/** The host's position at page time `now`, in ms. */
export function hostPositionMs(sample: HostSample, now: number): number {
  if (sample.paused) return sample.positionMs;
  return Math.max(0, sample.positionMs + (sample.ageMs + now - sample.receivedAt) * sample.rate);
}

export interface DriftCheck {
  /** Where this player should be (host position plus the guest's offset). */
  expectedMs: number;
  actualMs: number;
  actualPaused: boolean;
  hostPaused: boolean;
  /** Time since the last correction finished. */
  sinceAlignMs: number;
}

/** True when the guest's player should be brought back to the host. */
export function needsAlign(check: DriftCheck): boolean {
  const off = Math.abs(check.expectedMs - check.actualMs) > DEAD_BAND_MS;
  if (check.hostPaused) return !check.actualPaused || off;
  if (check.actualPaused) return true;
  return check.sinceAlignMs >= QUIET_AFTER_ALIGN_MS && off;
}

/** Signed difference of the guest's length to the host's (ms), or null when they match or one is unknown. */
export function durationDifferenceMs(hostMs: number | null | undefined, ownSeconds: number): number | null {
  if (!hostMs || !(ownSeconds > 0) || !Number.isFinite(ownSeconds)) return null;
  const diff = Math.round(ownSeconds * 1000 - hostMs);
  return Math.abs(diff) > DURATION_MISMATCH_MS ? diff : null;
}

/** The offset after one step up or down, kept within ±`MAX_OFFSET_MS`. */
export function stepOffset(offsetMs: number, direction: 1 | -1): number {
  return Math.max(-MAX_OFFSET_MS, Math.min(MAX_OFFSET_MS, offsetMs + direction * OFFSET_STEP_MS));
}

/** The digits of a typed code when it has exactly six ("123 456" and "123-456" work too). */
export function normalizeCode(input: string): string | null {
  const digits = input.replace(/[\s-]/g, '');
  return /^[0-9]{6}$/.test(digits) ? digits : null;
}

/** The translation key for a party command's error (`party:<kind>`), or null for others. */
export function partyErrorKey(error: unknown): string | null {
  const m = /^party:([a-z_]+)$/.exec(String(error ?? '').trim());
  return m ? `party.errors.${m[1]}` : null;
}
