import type { TKey } from '../i18n';

/** Anime4K upscaling presets of the embedded mpv engine (shaders bundled, MIT, by bloc97). */
export type Anime4kPreset = 'off' | 'a_fast' | 'b_fast' | 'c_fast' | 'a_hq' | 'b_hq' | 'c_hq';

export const ANIME4K_PRESETS: readonly Anime4kPreset[] = ['off', 'a_fast', 'b_fast', 'c_fast', 'a_hq', 'b_hq', 'c_hq'];

export type Anime4kMode = 'off' | 'a' | 'b' | 'c';
export type Anime4kQuality = 'fast' | 'hq';

// `label` is the mode's letter; 'Off' is shown through `common.off`.
export const ANIME4K_MODES: { value: Anime4kMode; label: string; hintKey: TKey }[] = [
  { value: 'off', label: 'Off', hintKey: 'anime4k.offHint' },
  { value: 'a', label: 'A', hintKey: 'anime4k.aHint' },
  { value: 'b', label: 'B', hintKey: 'anime4k.bHint' },
  { value: 'c', label: 'C', hintKey: 'anime4k.cHint' },
];

export function anime4kPreset(mode: Anime4kMode, quality: Anime4kQuality): Anime4kPreset {
  return mode === 'off' ? 'off' : `${mode}_${quality}`;
}

export function anime4kParts(preset: Anime4kPreset): { mode: Anime4kMode; quality: Anime4kQuality } {
  if (preset === 'off') return { mode: 'off', quality: 'fast' };
  const [mode, quality] = preset.split('_') as [Anime4kMode, Anime4kQuality];
  return { mode, quality };
}
