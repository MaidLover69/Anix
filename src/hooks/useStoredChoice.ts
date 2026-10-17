import { useCallback, useState } from 'react';

const PREFIX = 'anix-choice:';

function read<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const value = window.localStorage.getItem(PREFIX + key);
    return value !== null && (allowed as readonly string[]).includes(value) ? (value as T) : fallback;
  } catch {
    return fallback;
  }
}

/** A choice from a fixed set (a layout, a sort order) that is remembered across launches. */
export function useStoredChoice<T extends string>(key: string, allowed: readonly T[], fallback: T) {
  const [value, setValue] = useState<T>(() => read(key, allowed, fallback));
  const update = useCallback(
    (next: T) => {
      setValue(next);
      try {
        window.localStorage.setItem(PREFIX + key, next);
      } catch {
        // Storage can be unavailable; the choice then lasts for this session only.
      }
    },
    [key]
  );
  return [value, update] as const;
}
