/**
 * Which episodes the download dialog queues, and the language line download rows show.
 * Kept free of React so a node script can check it.
 */
import { sourceLabel, versionLabel } from './languages';

export type EpisodePick = 'range' | 'unwatched';

export interface PickInput {
  pick: EpisodePick;
  /** The last episode that can be downloaded, or null when unknown. */
  total: number | null;
  /** The typed range (`pick: 'range'`); either may be null while being typed. */
  from: number | null;
  to: number | null;
  /** Episode numbers already watched, already saved (or queued), and filler. */
  watched: number[];
  saved: number[];
  filler: number[];
  skipFiller: boolean;
  /** Stop after `limit + 1` picks, enough to tell the batch is over the limit. */
  limit?: number;
}

const has = (list: number[], episode: number) => list.some((n) => Math.abs(n - episode) < 0.01);

/** The episode numbers to queue, in order. */
export function pickEpisodes(input: PickInput): number[] {
  let start = 1;
  let end = input.total ?? 0;
  if (input.pick === 'range') {
    if (input.from === null || input.to === null) return [];
    start = Math.max(1, Math.min(input.from, input.to));
    end = Math.max(input.from, input.to);
  }
  const limit = input.limit ?? Infinity;
  const episodes: number[] = [];
  // Without a known total `end` is whatever was typed, so the loop stops at the limit instead.
  for (let n = start; n <= end && episodes.length <= limit; n++) {
    if (input.pick === 'unwatched' && has(input.watched, n)) continue;
    if (input.skipFiller && has(input.filler, n)) continue;
    if (!has(input.saved, n)) episodes.push(n);
  }
  return episodes;
}

/** "From episode N": the episode after the last one watched, or null when there is none. */
export function nextUnwatched(watched: number[], total: number | null): number | null {
  const last = watched.reduce((max, n) => Math.max(max, Math.floor(n)), 0);
  if (last === 0) return null;
  return total === null || last < total ? last + 1 : null;
}

/** "Japanese audio · English subtitles" for a sub, "Hindi audio" for a dub. */
export function downloadLanguageLabel(record: {
  category: string;
  audio_lang?: string | null;
  sub_lang?: string | null;
}): string {
  if (record.category.toLowerCase() === 'dub') {
    return sourceLabel(record.audio_lang, 'dub');
  }
  // "{language} audio", the same wording as a dub's track.
  const audio = versionLabel(record.audio_lang || 'ja', 'dub');
  return record.sub_lang ? `${audio} · ${versionLabel(record.sub_lang, 'sub')}` : audio;
}

export type StorageState = 'ok' | 'tight' | 'full';

/** `full` blocks the download: the estimate does not fit in the free space. `tight` leaves less than the headroom. */
export function storageState(neededBytes: number, freeBytes: number, headroomBytes: number): StorageState {
  if (neededBytes > freeBytes) return 'full';
  return freeBytes - neededBytes < headroomBytes ? 'tight' : 'ok';
}
