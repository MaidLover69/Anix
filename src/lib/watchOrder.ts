/**
 * The watch order of a franchise, built from the relations of the details already fetched:
 * prequels back to the first season, sequels forward, and side stories after the entry they
 * belong to. Kept free of React so a node script can check it.
 */

export interface OrderNode {
  id: number;
  title: { romaji?: string | null; english?: string | null; userPreferred?: string | null } | null;
  coverImage?: { large?: string | null; extraLarge?: string | null } | null;
  format?: string | null;
  seasonYear?: number | null;
}

/** The part of a details answer the order needs. */
export interface OrderDetails extends OrderNode {
  relations?: { edges: { relationType: string | null; node: OrderNode | null }[] } | null;
}

export interface OrderEntry {
  node: OrderNode;
  /** 1-based place in the order. */
  number: number;
  side: boolean;
  current: boolean;
}

/** Manga, novels and other print entries are not watched. */
const PRINT_FORMATS = ['MANGA', 'NOVEL', 'ONE_SHOT'];
/** A long-running franchise stops here rather than fetching every entry. */
export const MAX_ENTRIES = 15;

const watchable = (node: OrderNode | null): node is OrderNode =>
  !!node && node.id !== 0 && !PRINT_FORMATS.includes((node.format ?? '').toUpperCase());

function related(details: OrderDetails | undefined, type: string): OrderNode[] {
  return (details?.relations?.edges ?? [])
    .filter((e) => e.relationType === type)
    .map((e) => e.node)
    .filter(watchable)
    // Of several, the earliest; a TV season before a same-year film.
    .sort((a, b) => (a.seasonYear ?? 9999) - (b.seasonYear ?? 9999) || Number(b.format === 'TV') - Number(a.format === 'TV'));
}

/**
 * The order around `currentId`, and the ids whose details are still needed to extend it
 * (the ends of the chain that have not been fetched). `details` maps id → fetched details.
 */
export function watchOrder(
  currentId: number,
  details: Map<number, OrderDetails>
): { entries: OrderEntry[]; missing: number[] } {
  const current = details.get(currentId);
  if (!current) return { entries: [], missing: [currentId] };
  const missing: number[] = [];
  const seen = new Set<number>([currentId]);

  const walk = (type: 'PREQUEL' | 'SEQUEL'): OrderNode[] => {
    const out: OrderNode[] = [];
    let at: OrderDetails | undefined = current;
    while (at && out.length < MAX_ENTRIES) {
      const next = related(at, type).find((n) => !seen.has(n.id));
      if (!next) break;
      seen.add(next.id);
      const full = details.get(next.id);
      out.push(full ?? next);
      if (!full) missing.push(next.id);
      at = full;
    }
    return out;
  };

  const prequels = walk('PREQUEL').reverse();
  const order = [...prequels, current, ...walk('SEQUEL')];
  // A long chain keeps the window around this anime, so "You're here" is never cut off.
  const start = Math.max(0, Math.min(prequels.length - Math.floor(MAX_ENTRIES / 2), order.length - MAX_ENTRIES));
  const chain = order.slice(start, start + MAX_ENTRIES);
  const entries: OrderEntry[] = [];
  for (const node of chain) {
    entries.push({ node, number: 0, side: false, current: node.id === currentId });
    for (const side of related(details.get(node.id), 'SIDE_STORY')) {
      if (seen.has(side.id)) continue;
      seen.add(side.id);
      entries.push({ node: side, number: 0, side: true, current: false });
    }
  }
  entries.forEach((entry, i) => (entry.number = i + 1));
  return { entries, missing };
}
