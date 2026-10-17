import { useCallback, useState } from 'react';

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

/** The last searches on Discover, newest first, kept on this computer only. */
export function useSearchHistory() {
  const [history, setHistory] = useState<string[]>(read);

  const update = useCallback((change: (list: string[]) => string[]) => {
    setHistory((prev) => {
      const next = change(prev);
      write(next);
      return next;
    });
  }, []);

  const record = useCallback((query: string) => update((list) => withSearch(list, query)), [update]);
  const remove = useCallback(
    (query: string) => update((list) => list.filter((q) => q !== query)),
    [update]
  );
  const clear = useCallback(() => update(() => []), [update]);

  return { history, record, remove, clear };
}
