/** Splitting a label into the parts a search matched, for `<mark>` highlighting (Settings search). */

export interface TextPart {
  text: string;
  /** True when this part matched a word of the query. */
  hit: boolean;
}

/**
 * `text` cut into matched and unmatched parts, in order; joined again they give `text` back.
 * Every word of `query` (any case) is marked wherever it appears.
 */
export function markParts(text: string, query: string): TextPart[] {
  const words = query
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((word) => word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  if (words.length === 0) return [{ text, hit: false }];
  // A capturing split puts the matches at the odd indices.
  return text
    .split(new RegExp(`(${words.join('|')})`, 'gi'))
    .map((part, index) => ({ text: part, hit: index % 2 === 1 }))
    .filter((part) => part.text !== '');
}
