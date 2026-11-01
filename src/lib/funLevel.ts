/**
 * Fun mode's game numbers, worked out from data the app already has: a player level from the
 * episodes watched (1 EXP each), a rank title per level band, and a "power level" from a score.
 * Pure, so `scripts/routes-test.mjs` can check it.
 */

/** Rank titles from the lowest band up; the i18n key is `fun.rank.<id>`. */
export const RANKS = ['rookie', 'otaku', 'senpai', 'sensei', 'legend'] as const;
export type Rank = (typeof RANKS)[number];

/** The first level of each rank in `RANKS`. */
const RANK_FROM_LEVEL = [1, 5, 10, 20, 35];

/** Total EXP that reaching `level` takes: 5·n², a gentle curve (level 2 at 20 episodes, 10 at 500). */
export function expForLevel(level: number): number {
  return level <= 1 ? 0 : 5 * level * level;
}

export interface PlayerLevel {
  level: number;
  rank: Rank;
  /** EXP gained inside this level, and what the whole level takes. */
  intoLevel: number;
  levelSize: number;
}

export function rankFor(level: number): Rank {
  let rank: Rank = RANKS[0];
  RANK_FROM_LEVEL.forEach((from, i) => {
    if (level >= from) rank = RANKS[i];
  });
  return rank;
}

export function playerLevel(exp: number): PlayerLevel {
  const total = Number.isFinite(exp) && exp > 0 ? Math.floor(exp) : 0;
  let level = Math.max(1, Math.floor(Math.sqrt(total / 5)));
  // Guards the square root against rounding at exact boundaries.
  while (expForLevel(level + 1) <= total) level += 1;
  while (level > 1 && expForLevel(level) > total) level -= 1;
  const floor = expForLevel(level);
  return { level, rank: rankFor(level), intoLevel: total - floor, levelSize: expForLevel(level + 1) - floor };
}

export interface PowerLevel {
  /** The score (0–100) × 100. */
  value: number;
  /** 0–100, for the meter. */
  percent: number;
  over9000: boolean;
}

/** From the 0–100 score, or else MyAnimeList's 0–10 one; null when neither is known. */
export function powerLevel(percentScore: number | null | undefined, malScore: number | null | undefined): PowerLevel | null {
  const score =
    typeof percentScore === 'number' && percentScore > 0
      ? percentScore
      : typeof malScore === 'number' && malScore > 0
        ? malScore * 10
        : null;
  if (score === null) return null;
  const percent = Math.min(100, Math.round(score * 10) / 10);
  return { value: Math.round(percent * 100), percent, over9000: percent >= 90 };
}
