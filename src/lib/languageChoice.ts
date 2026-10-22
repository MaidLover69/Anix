/** The first/second/third-choice model over an ordered language list. No imports (node tests load it). */

/** How many choices the pickers show; older saved lists may hold more, which are kept after them. */
export const LANGUAGE_SLOTS = 3;

/**
 * The list after choice `slot` (0 = first) became `code` ('' = none): a code chosen in another
 * slot trades places with the old one, empty slots close up, and entries past the slots stay.
 * The first choice is never emptied.
 */
export function withChoice(list: string[], slot: number, code: string): string[] {
  if (slot === 0 && code === '') return list;
  const slots = Array.from({ length: LANGUAGE_SLOTS }, (_, i) => list[i] ?? '');
  const rest = list.slice(LANGUAGE_SLOTS);
  const other = slots.indexOf(code);
  if (code !== '' && other !== -1) slots[other] = slots[slot];
  slots[slot] = code;
  const chosen = slots.filter((value, i) => value !== '' && slots.indexOf(value) === i);
  return [...chosen, ...rest.filter((value) => !chosen.includes(value))];
}
