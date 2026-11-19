/** The exact `search_anime` arguments for a set of filters, shared by Discover, the home rows
 *  and smart collections so one rule gives one query (and one saved copy). Empty means null. */
export interface SearchArgsInput {
  query?: string;
  genres?: string[];
  format?: string;
  status?: string;
  sort?: string;
  page?: number;
  perPage?: number;
  excludeGenres?: string[];
  /** Lowest average score, 0–100. */
  minScore?: number;
  year?: number;
  /** WINTER, SPRING, SUMMER or FALL. */
  season?: string;
  studio?: string;
}

export function searchInvokeArgs(params: SearchArgsInput): Record<string, unknown> {
  return {
    query: params.query || null,
    genres: params.genres && params.genres.length > 0 ? params.genres : null,
    format: params.format || null,
    status: params.status || null,
    sort: params.sort || 'POPULARITY_DESC',
    page: params.page || 1,
    perPage: params.perPage || 24,
    excludeGenres: params.excludeGenres && params.excludeGenres.length > 0 ? params.excludeGenres : null,
    minScore: params.minScore || null,
    year: params.year || null,
    season: params.season || null,
    studio: params.studio?.trim() || null,
  };
}
