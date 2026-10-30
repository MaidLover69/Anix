import type { HistoryEntry } from '../types';

/** An episode counts as watched from this fraction on (same threshold as the backend). */
export const WATCHED_FRACTION = 0.85;

/** Saved position of a history row as 0..100. */
export function historyProgressPercent(entry: HistoryEntry): number {
  if (!(entry.duration_ms > 0)) return 0;
  return Math.min(100, Math.max(0, (entry.position_ms / entry.duration_ms) * 100));
}

export function isHistoryEntryFinished(entry: HistoryEntry): boolean {
  return entry.duration_ms > 0 && entry.position_ms / entry.duration_ms >= WATCHED_FRACTION;
}

/** History counts an episode only after this much real playback: 10 s or 2 % of it, whichever comes first. */
export const HISTORY_MIN_PLAYED_SECONDS = 10;
export const HISTORY_MIN_PLAYED_FRACTION = 0.02;
/** A forward step between two time readings above this is a jump (seek, skip), not playback. */
const MAX_PLAYED_STEP_SECONDS = 3;

/** Seconds of playback between two readings of the media time while it plays; jumps and steps back count 0. */
export function playedStep(previous: number, current: number): number {
  const step = current - previous;
  return Number.isFinite(step) && step > 0 && step <= MAX_PLAYED_STEP_SECONDS ? step : 0;
}

/** Whether `playedSeconds` of real playback is enough for a history entry (needs a known duration). */
export function hasPlayedEnough(playedSeconds: number, duration: number): boolean {
  if (!(duration > 0)) return false;
  return playedSeconds >= Math.min(HISTORY_MIN_PLAYED_SECONDS, duration * HISTORY_MIN_PLAYED_FRACTION);
}

/**
 * Number of episodes that have aired, or null when it cannot be told.
 * `nextAiringEpisode` is the number of the next episode still to air.
 */
export function releasedEpisodeCount(
  status: string | null | undefined,
  totalEpisodes: number | null | undefined,
  nextAiringEpisode: number | null | undefined
): number | null {
  if (status === 'NOT_YET_RELEASED') return 0;
  if (nextAiringEpisode && nextAiringEpisode > 0) return nextAiringEpisode - 1;
  if (totalEpisodes && totalEpisodes > 0) return totalEpisodes;
  return null;
}

export interface ContinueTarget {
  episode: number;
  /** True when `episode` is a partly watched episode to resume, not a fresh one. */
  resume: boolean;
  category: 'sub' | 'dub' | null;
  provider: string | null;
}

interface ContinueTargetInput {
  /** Most recent history row for this anime, if any. */
  latest?: HistoryEntry | null;
  /** Episode numbers watched to the end. */
  watched?: number[];
  /** Released episode numbers when a list is known (any order). */
  episodeNumbers?: number[];
  /** Released episode count, used when no list is known. */
  released?: number | null;
}

/**
 * Where "Continue" should go: the most recent episode if it is unfinished, otherwise the
 * next released episode after the highest one watched. Never points past what has aired.
 * Returns null when nothing has been watched yet.
 */
export function getContinueTarget({
  latest,
  watched = [],
  episodeNumbers = [],
  released,
}: ContinueTargetInput): ContinueTarget | null {
  const category = latest?.category === 'dub' ? 'dub' : latest?.category === 'sub' ? 'sub' : null;
  const provider = latest?.provider || null;

  if (latest && !isHistoryEntryFinished(latest)) {
    return { episode: latest.episode_number, resume: true, category, provider };
  }

  const seen = latest ? [...watched, latest.episode_number] : watched;
  if (seen.length === 0) return null;
  const highest = Math.max(...seen);

  const sorted = [...episodeNumbers].sort((a, b) => a - b);
  if (sorted.length > 0) {
    const next = sorted.find((n) => n > highest + 0.001);
    return { episode: next ?? sorted[sorted.length - 1], resume: false, category, provider };
  }

  const next = Math.floor(highest) + 1;
  const episode = released !== null && released !== undefined && released > 0 ? Math.min(next, released) : next;
  return { episode, resume: false, category, provider };
}

/** Route to the player for an episode, with the audio and provider choice in the query. */
export function watchPath(
  anilistId: number,
  episode: number,
  category?: string | null,
  provider?: string | null
): string {
  const params = new URLSearchParams();
  params.set('cat', category === 'dub' ? 'dub' : 'sub');
  params.set('prov', provider || 'auto');
  return `/watch/${anilistId}/${episode}?${params.toString()}`;
}

/** An episode's own title, or null when it is missing or only repeats "Episode N". */
export function distinctEpisodeTitle(
  episodeNumber: number,
  title: string | null | undefined
): string | null {
  const trimmed = title?.trim();
  if (!trimmed) return null;
  const generic = new RegExp(`^(episode|ep\\.?)\\s*0*${episodeNumber}$`, 'i');
  return generic.test(trimmed) ? null : trimmed;
}
