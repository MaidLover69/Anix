/**
 * Smart collections: a collection that fills itself from a saved set of Discover filters
 * (genres, left-out genres, format, status, season, year, minimum score, sort, search text).
 * The rule is stored as a Discover query string, so a smart collection shows exactly what
 * Discover shows for the same filters. Kept free of React so a node script can check it.
 */

/** The Discover parameters a rule keeps, in the order they are written. */
const SINGLE_KEYS = ['q', 'format', 'status', 'season', 'year', 'score', 'studio', 'sort'] as const;
const LIST_KEYS = ['genre', 'xgenre'] as const;
const MAX_VALUE_CHARS = 80;
const MAX_GENRES = 12;
/** Upper case words with underscores (AniList enums such as TV_SHORT, SCORE_DESC). */
const ENUM = /^[A-Z][A-Z0-9_]{0,30}$/;

/** What `search_anime` takes (the same shape as `useSearchAnime`'s parameters). */
export interface RuleSearch {
  query?: string;
  genres?: string[];
  excludeGenres?: string[];
  format?: string;
  status?: string;
  season?: string;
  year?: number;
  minScore?: number;
  sort?: string;
  studio?: string;
}

const clean = (value: string | null) => {
  const text = (value ?? '').trim();
  return text && text.length <= MAX_VALUE_CHARS ? text : '';
};

/**
 * The rule for the current Discover filters, or null when no filter is set (a rule that is only
 * a sort order would be the whole catalogue). Unknown parameters such as the page are left out.
 */
export function ruleFromSearch(params: URLSearchParams): string | null {
  const out = new URLSearchParams();
  for (const key of LIST_KEYS) {
    for (const value of params.getAll(key).map(clean).filter(Boolean).slice(0, MAX_GENRES)) out.append(key, value);
  }
  for (const key of SINGLE_KEYS) {
    const value = clean(params.get(key));
    if (value) out.set(key, value);
  }
  const filters = [...out.keys()].filter((key) => key !== 'sort');
  return filters.length > 0 ? out.toString() : null;
}

/** The search a rule stands for. Values that cannot be valid are dropped, never passed on. */
export function searchFromRule(rule: string): RuleSearch {
  const params = new URLSearchParams(rule);
  const genres = params.getAll('genre').map(clean).filter(Boolean).slice(0, MAX_GENRES);
  const excluded = params
    .getAll('xgenre')
    .map(clean)
    .filter((g) => g && !genres.includes(g))
    .slice(0, MAX_GENRES);
  const enumOf = (key: string) => {
    const value = clean(params.get(key));
    return ENUM.test(value) ? value : undefined;
  };
  const studio = clean(params.get('studio'));
  const year = Number(params.get('year'));
  const score = Number(params.get('score'));
  return {
    query: clean(params.get('q')) || undefined,
    genres: genres.length ? genres : undefined,
    excludeGenres: excluded.length ? excluded : undefined,
    format: enumOf('format'),
    status: enumOf('status'),
    season: enumOf('season'),
    year: Number.isInteger(year) && year >= 1900 && year <= 3000 ? year : undefined,
    minScore: Number.isInteger(score) && score >= 1 && score <= 100 ? score : undefined,
    sort: enumOf('sort'),
    // Left out when unset, so a rule without a studio gives the same object as before 1.4.2.
    ...(studio ? { studio } : {}),
  };
}

/** The Discover link that shows the same titles as the rule. */
export function discoverLink(rule: string): string {
  return `/discover?${rule}`;
}

/** The largest id a franchise rule may name (AniList ids and stand-ins are far below it). */
const MAX_ID = 1e12;

/** The rule of a franchise collection: the watch order of the franchise this anime belongs to. */
export function ruleForFranchise(id: number): string {
  if (!Number.isInteger(id) || id <= 0 || id > MAX_ID) throw new Error('A franchise rule needs a positive anime id');
  return `franchise=${id}`;
}

/** The anime a franchise rule names, or null for any other rule (or an id that cannot be real). */
export function franchiseFromRule(rule: string): number | null {
  const value = new URLSearchParams(rule).get('franchise');
  if (!value || !/^\d+$/.test(value)) return null;
  const id = Number(value);
  return id > 0 && id <= MAX_ID ? id : null;
}
