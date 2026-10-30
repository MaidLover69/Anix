/**
 * Which anime ids and episode numbers the routes, deep links and notification clicks accept.
 * Pure, so `scripts/routes-test.mjs` can check them.
 */

/** AniList ids are 32-bit, and stand-ins (`-(site × 100_000_000 + id)`, see the backend's `catalogue/ids.rs`) fit too. */
const MAX_ANIME_ID = 2_147_483_647;

/** An anime id as the backend knows it: a positive AniList id or a negative stand-in; 0 is no anime. */
export function isAnimeId(id: number): boolean {
  return Number.isInteger(id) && id !== 0 && Math.abs(id) <= MAX_ANIME_ID;
}

/** The id in a route or link segment (`154587`, `-300000099`), or null. */
export function parseAnimeId(text: string | null | undefined): number | null {
  if (!text || !/^-?[1-9]\d{0,9}$/.test(text)) return null;
  const id = Number(text);
  return isAnimeId(id) ? id : null;
}

/** `anix://anime/<id>` (or the older `anilili://…`) → the id, or null for anything else. */
export function parseDeepLink(url: string): number | null {
  const match = url.trim().match(/^(?:anix|anilili):\/\/anime\/([^/?#]+)\/?(?:[?#].*)?$/i);
  return match ? parseAnimeId(match[1]) : null;
}

/** The anime of an `open-anime` event (a clicked release alert or tray item), or null. */
export function openAnimeId(payload: unknown): number | null {
  const id = Number(payload);
  return typeof payload === 'number' && isAnimeId(id) ? id : null;
}

export interface KnownEpisodes {
  /** The planned episode count, when the catalogue knows it. */
  total?: number | null;
  /** Episodes released so far (the next airing episode − 1); may lag behind the sites. */
  aired?: number | null;
  /** Still airing: a missing total then means "more to come", not "no limit known". */
  airing?: boolean;
  /** Episode numbers some list has (extensions, catalogue, the user's files). */
  listed?: number[];
}

/**
 * Whether episode `n` can exist. A number some list has always can (episode 0 or 12.5 of
 * some shows). Otherwise it must be a whole number ≥ 1 and, when a count is known, within
 * max(total, aired, listed). An airing show without a total has no upper limit: its
 * next-airing count can be older than what the sites already have.
 */
export function episodeExists(n: number, known: KnownEpisodes = {}): boolean {
  const listed = known.listed ?? [];
  if (listed.some((m) => Math.abs(m - n) < 0.01)) return true;
  if (!Number.isInteger(n) || n < 1) return false;
  const total = known.total && known.total > 0 ? known.total : null;
  if (total === null && known.airing) return true;
  const limit = Math.max(total ?? 0, known.aired ?? 0, ...listed);
  return limit <= 0 || n <= limit;
}
