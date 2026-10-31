/**
 * The live state of each extension during a source lookup, from the backend's
 * `sources-progress` event, and the short summary shown under the loading spinner.
 */

export type LookupState = 'asking' | 'failed' | 'found';

export interface LookupProgress {
  anilist_id: number;
  episode: number;
  category: string;
  provider: string;
  state: LookupState;
}

export interface LookupKey {
  anilistId: number;
  episode: number;
  category: string;
}

/** Extension id → its latest state, in the order they were first asked. */
export type ProgressList = [string, LookupState][];

/** The list after one event. Events of another lookup (title, episode or audio) are ignored. */
export function applyProgress(list: ProgressList, event: LookupProgress, key: LookupKey): ProgressList {
  if (event.anilist_id !== key.anilistId || event.episode !== key.episode || event.category !== key.category) return list;
  const at = list.findIndex(([id]) => id === event.provider);
  if (at === -1) return [...list, [event.provider, event.state]];
  if (list[at][1] === event.state) return list;
  const next = list.slice();
  next[at] = [event.provider, event.state];
  return next;
}

/** At most this many names are listed; the rest are counted. */
export const MAX_NAMES = 4;

export function progressSummary(list: ProgressList): { asking: string[]; more: number; failed: number } {
  const asking = list.filter(([, state]) => state === 'asking').map(([id]) => id);
  return {
    asking: asking.slice(0, MAX_NAMES),
    more: Math.max(0, asking.length - MAX_NAMES),
    failed: list.filter(([, state]) => state === 'failed').length,
  };
}
