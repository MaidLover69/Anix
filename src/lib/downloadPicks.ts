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
}

const has = (list: number[], episode: number) => list.some((n) => Math.abs(n - episode) < 0.01);

/** The episode numbers to queue, in order. */
export function pickEpisodes(input: PickInput): number[] {
  let episodes: number[] = [];
  if (input.pick === 'unwatched') {
    for (let n = 1; n <= (input.total ?? 0); n++) if (!has(input.watched, n)) episodes.push(n);
  } else if (input.from !== null && input.to !== null) {
    const start = Math.max(1, Math.min(input.from, input.to));
    const end = Math.max(input.from, input.to);
    for (let n = start; n <= end; n++) episodes.push(n);
  }
  if (input.skipFiller) episodes = episodes.filter((n) => !has(input.filler, n));
  return episodes.filter((n) => !has(input.saved, n));
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
