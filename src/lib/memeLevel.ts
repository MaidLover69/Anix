/**
 * Settings › Appearance › Meme mode: Low (the normal app, default), Medium (the game-style layer
 * that was "Fun mode" before 1.4) or High (meme images and lines). Pure, so
 * `scripts/routes-test.mjs` can check how older settings load.
 */

export type MemeLevel = 'low' | 'medium' | 'high';
export const MEME_LEVELS: readonly MemeLevel[] = ['low', 'medium', 'high'];

export function isMemeLevel(value: unknown): value is MemeLevel {
  return typeof value === 'string' && (MEME_LEVELS as readonly string[]).includes(value);
}

/** A valid `meme_mode` wins; no `meme_mode` and the old `fun_mode: true` → 'medium'; anything else → 'low'. */
export function memeLevelOf(stored: { meme_mode?: unknown; fun_mode?: unknown }): MemeLevel {
  if ('meme_mode' in stored) return isMemeLevel(stored.meme_mode) ? stored.meme_mode : 'low';
  return stored.fun_mode === true ? 'medium' : 'low';
}
