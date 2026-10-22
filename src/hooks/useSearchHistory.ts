import { useSyncExternalStore } from 'react';

const STORAGE_KEY = 'anix-search-history';
export const SEARCH_HISTORY_LIMIT = 10;

function read(): string[] {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '[]');
    return Array.isArray(parsed)
      ? parsed.filter((q): q is string => typeof q === 'string' && q.trim() !== '').slice(0, SEARCH_HISTORY_LIMIT)
      : [];
  } catch {
    return [];
  }
}

function write(list: string[]) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
  } catch {
    // Storage can be unavailable; the history then lasts for this session only.
  }
}

/** `query` first, without an earlier copy that differs only in letter case or spacing. */
export function withSearch(list: string[], query: string): string[] {
  const trimmed = query.trim().replace(/\s+/g, ' ');
  if (!trimmed) return list;
  const key = trimmed.toLowerCase();
  return [trimmed, ...list.filter((q) => q.toLowerCase() !== key)].slice(0, SEARCH_HISTORY_LIMIT);
}

// One list for every screen that shows it (Discover, the Ctrl+K palette), so a change in
// one shows in the other at once.
let current: string[] | null = null;
const listeners = new Set<() => void>();

function snapshot(): string[] {
  if (current === null) current = read();
  return current;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function update(change: (list: string[]) => string[]) {
  current = change(snapshot());
  write(current);
  listeners.forEach((listener) => listener());
}

const record = (query: string) => update((list) => withSearch(list, query));
const remove = (query: string) => update((list) => list.filter((q) => q !== query));
const clear = () => update(() => []);

/** The last searches, newest first, kept on this computer only. */
export function useSearchHistory() {
  const history = useSyncExternalStore(subscribe, snapshot);
  return { history, record, remove, clear };
}
