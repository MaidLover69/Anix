import { useCallback } from 'react';
import { AUTO_PROVIDER, useSaveSettings, useSettings } from './useSettings';
import type { RememberedSource } from '../types';

/** What an anime's episode links open with: the server last picked for it, else the settings. */
export function openWith(
  remembered: RememberedSource | null,
  accepts: (key: string) => boolean,
  preferDub: boolean | undefined,
  preferredProvider: string | null | undefined
): { category: 'sub' | 'dub'; provider: string } {
  if (remembered && accepts(remembered.provider)) return { category: remembered.category, provider: remembered.provider };
  return { category: preferDub ? 'dub' : 'sub', provider: preferredProvider || AUTO_PROVIDER };
}

/** Anime remembered at most; the ones picked longest ago make room. */
const MAX_REMEMBERED = 300;

/** The map with `anilistId`'s server set to this one, trimmed to the most recent entries. */
export function withRemembered(
  map: Record<string, RememberedSource> | undefined,
  anilistId: number,
  source: Omit<RememberedSource, 'at'>,
  now: number
): Record<string, RememberedSource> {
  const next: Record<string, RememberedSource> = { ...(map ?? {}), [String(anilistId)]: { ...source, at: now } };
  const keys = Object.keys(next);
  if (keys.length <= MAX_REMEMBERED) return next;
  keys
    .sort((a, b) => next[a].at - next[b].at)
    .slice(0, keys.length - MAX_REMEMBERED)
    .forEach((key) => delete next[key]);
  return next;
}

/**
 * The server (provider and audio) last picked for an anime, so its next episodes try it first.
 * Kept in the settings file, per AniList id.
 */
export function useRememberedSource(anilistId: number) {
  const settings = useSettings().data;
  const { mutate } = useSaveSettings();
  const remembered = settings?.remembered_sources?.[String(anilistId)] ?? null;
  const map = settings?.remembered_sources;

  const remember = useCallback(
    (provider: string, category: 'sub' | 'dub') => {
      const known = map?.[String(anilistId)];
      if (known && known.provider === provider && known.category === category) return;
      mutate({ remembered_sources: withRemembered(map, anilistId, { provider, category }, Date.now()) });
    },
    [anilistId, map, mutate]
  );

  return { remembered, remember };
}
