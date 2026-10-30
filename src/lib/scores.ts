/**
 * The scores the anime page shows, each credited to the catalogue it came from and each
 * source once. Pure, so `scripts/routes-test.mjs` can check it.
 */

/** Catalogues whose score is MyAnimeList's own: Jikan reads MyAnimeList, Shikimori copies its score. */
const MAL_SCORED = new Set(['MyAnimeList', 'Jikan', 'Shikimori']);

export interface DetailScores {
  /** The details' `averageScore` (0–100) and who gave it, when shown. */
  percent: { source: string; score: number } | null;
  /** MyAnimeList's score (0–10), when shown. */
  mal: number | null;
}

/**
 * `averageScore` comes from whichever catalogue served the details (`servedBy`, AniList when
 * unset). When that is MyAnimeList's own score it is credited to MyAnimeList, and left out
 * when MyAnimeList's score is shown too, since it is the same number.
 */
export function detailScores(
  averageScore: number | null | undefined,
  servedBy: string | null | undefined,
  malScore: number | null | undefined
): DetailScores {
  const mal = typeof malScore === 'number' && malScore > 0 ? malScore : null;
  const servedFrom = servedBy || 'AniList';
  const source = MAL_SCORED.has(servedFrom) ? 'MyAnimeList' : servedFrom;
  const percent =
    averageScore && averageScore > 0 && !(mal !== null && source === 'MyAnimeList') ? { source, score: averageScore } : null;
  return { percent, mal };
}
