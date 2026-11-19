/** The covers a collection's collage shows: the first `max` distinct, non-blank addresses, in
 *  the order given. The input is left as it is. */
export function collageCovers(covers: (string | null | undefined)[] | null | undefined, max = 4): string[] {
  const out: string[] = [];
  for (const cover of covers ?? []) {
    if (out.length >= max) break;
    if (cover && cover.trim() && !out.includes(cover)) out.push(cover);
  }
  return out;
}
