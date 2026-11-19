// Playback speed rules, pure so node can check them (no imports).

/** `[` and `]` change the speed in these steps, within these bounds. */
export const SPEED_STEP = 0.25;
export const MIN_SPEED = 0.25;
export const MAX_SPEED = 3;

/** The speed menu's fixed steps. Above 3×, only the custom speed reaches. */
export const FIXED_SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];

/** The custom speed (and the default speed setting): 0.5× to 4× in 0.05 steps. */
export const CUSTOM_SPEED_MIN = 0.5;
export const CUSTOM_SPEED_MAX = 4;
export const CUSTOM_SPEED_STEP = 0.05;

export function stepSpeed(rate: number, direction: 1 | -1): number {
  const current = Number.isFinite(rate) && rate > 0 ? rate : 1;
  // Snap to the grid first, so 1.1 goes to 1.25 / 1, not 1.35 / 0.85.
  const snapped = direction > 0 ? Math.floor(current / SPEED_STEP + 1e-9) : Math.ceil(current / SPEED_STEP - 1e-9);
  const next = (snapped + direction) * SPEED_STEP;
  return Math.min(MAX_SPEED, Math.max(MIN_SPEED, next));
}

export function isFixedSpeed(rate: number): boolean {
  return FIXED_SPEEDS.includes(rate);
}

/** A custom speed from a number or text, rounded to the 0.05 grid; null when outside 0.5–4 or not a number. */
export function parseCustomSpeed(input: unknown): number | null {
  if (typeof input === 'string' && input.trim() === '') return null;
  if (typeof input !== 'number' && typeof input !== 'string') return null;
  const value = Number(input);
  if (!Number.isFinite(value) || value < CUSTOM_SPEED_MIN || value > CUSTOM_SPEED_MAX) return null;
  return Number((Math.round(value / CUSTOM_SPEED_STEP) * CUSTOM_SPEED_STEP).toFixed(2));
}

/** Every episode starts at the default speed setting, never at the speed of the last one. */
export function episodeStartRate(defaultSetting: unknown): number {
  return parseCustomSpeed(defaultSetting) ?? 1;
}

export type SpeedSpot = 'spd05' | 'spd075' | 'spd125' | 'spd15' | 'spd2' | 'spd3' | 'spdCustom';

const FIXED_SPOTS: Record<number, SpeedSpot> = {
  0.5: 'spd05', 0.75: 'spd075', 1.25: 'spd125', 1.5: 'spd15', 2: 'spd2', 3: 'spd3',
};

/** The Meme mode caption of a speed; none for 1×. */
export function speedSpot(rate: number): SpeedSpot | null {
  return rate === 1 ? null : FIXED_SPOTS[rate] ?? 'spdCustom';
}
