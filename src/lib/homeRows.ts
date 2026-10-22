/**
 * The rows of the Home screen and the order chosen for them in Settings › Appearance.
 * Kept free of React so a node script can check it; labels are i18n keys.
 */

export const HOME_ROWS = [
  { id: 'continue', labelKey: 'homeRows.continue' },
  { id: 'because', labelKey: 'homeRows.because' },
  { id: 'trending', labelKey: 'homeRows.trending' },
  { id: 'popular', labelKey: 'homeRows.popular' },
  { id: 'top_rated', labelKey: 'homeRows.topRated' },
] as const;

export type HomeRowId = (typeof HOME_ROWS)[number]['id'];

const KNOWN = HOME_ROWS.map((row) => row.id) as HomeRowId[];

/**
 * The rows in the saved order. Unknown ids are dropped; rows the saved order does not name
 * (new in this version) follow in their default order.
 */
export function orderHomeRows(saved: readonly string[] | null | undefined): HomeRowId[] {
  const picked = (saved ?? []).filter((id, i, all): id is HomeRowId => KNOWN.includes(id as HomeRowId) && all.indexOf(id) === i);
  return [...picked, ...KNOWN.filter((id) => !picked.includes(id))];
}

/** `order` with `id` moved one place up (-1) or down (+1); unchanged at either end. */
export function moveHomeRow(order: readonly HomeRowId[], id: HomeRowId, step: -1 | 1): HomeRowId[] {
  const from = order.indexOf(id);
  const to = from + step;
  if (from < 0 || to < 0 || to >= order.length) return [...order];
  const next = [...order];
  [next[from], next[to]] = [next[to], next[from]];
  return next;
}
