/**
 * A `Media` with only what an anime card shows (id, title, cover, format, score, year), for
 * rows built from the watchlist, the schedule or a playlist instead of a catalogue answer.
 */
import type { Media } from '../types';

export interface CardFields {
  id: number;
  title: string;
  cover: string | null | undefined;
  format?: string | null;
  score?: number | null;
}

export function cardMedia({ id, title, cover, format = null, score = null }: CardFields): Media {
  return {
    id,
    id_mal: null,
    title: { romaji: title, english: title, native: null, user_preferred: title },
    cover_image: { extra_large: null, large: cover ?? null, color: null },
    banner_image: null,
    description: null,
    format,
    status: null,
    episodes: null,
    duration: null,
    season: null,
    season_year: null,
    average_score: score,
    mean_score: null,
    popularity: null,
    favourites: null,
    genres: [],
    is_adult: false,
  };
}

/** One card per anime, in first-seen order (a playlist holds episodes, often several per title). */
export function uniqueCards<T>(entries: readonly T[], fields: (entry: T) => CardFields): Media[] {
  const seen = new Set<number>();
  const out: Media[] = [];
  for (const entry of entries) {
    const card = fields(entry);
    if (seen.has(card.id)) continue;
    seen.add(card.id);
    out.push(cardMedia(card));
  }
  return out;
}
