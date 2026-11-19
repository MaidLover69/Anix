export type SavedAgeUnit = 'now' | 'minutes' | 'hours' | 'days';

/** How old a saved copy is, in the largest whole unit; under a minute, or a clock that went
 *  backwards, reads as "now". */
export function savedAge(ageSeconds: number): { unit: SavedAgeUnit; count: number } {
  if (!Number.isFinite(ageSeconds) || ageSeconds < 60) return { unit: 'now', count: 0 };
  if (ageSeconds < 3600) return { unit: 'minutes', count: Math.floor(ageSeconds / 60) };
  if (ageSeconds < 86400) return { unit: 'hours', count: Math.floor(ageSeconds / 3600) };
  return { unit: 'days', count: Math.floor(ageSeconds / 86400) };
}
