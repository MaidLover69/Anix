import { useMemo } from 'react';
import type { QueryClient } from '@tanstack/react-query';
import { DEFAULT_SETTINGS, useSettings } from './useSettings';
import { DEFAULT_LANGUAGES } from '../lib/languages';

export interface LanguagePrefs {
  /** Subtitle languages for subs, most wanted first. Never empty. */
  sub: string[];
  /** Audio languages for dubs, most wanted first. Never empty. */
  dub: string[];
  /** Auto only asks plugins in these languages. */
  strict: boolean;
}

function codes(value: unknown): string[] {
  const list = Array.isArray(value)
    ? value.filter((code): code is string => typeof code === 'string' && code.trim() !== '')
    : [];
  return list.length > 0 ? list.map((code) => code.trim().toLowerCase()) : DEFAULT_LANGUAGES;
}

/** The language preferences as saved; the backend reads the same keys. */
export function useLanguagePrefs(): LanguagePrefs {
  const settings = useSettings().data ?? DEFAULT_SETTINGS;
  const { preferred_sub_languages: sub, preferred_dub_languages: dub, only_preferred_languages: strict } = settings;
  return useMemo(
    () => ({ sub: codes(sub), dub: codes(dub), strict: strict !== false }),
    [sub, dub, strict]
  );
}

/**
 * After a language change: provider episode lists are filtered by language, and streams
 * resolved earlier may be in a language no longer wanted. The stream on screen is left alone.
 */
export function refreshAfterLanguageChange(queryClient: QueryClient) {
  queryClient.invalidateQueries({ queryKey: ['episodes'] });
  queryClient.removeQueries({ queryKey: ['episodeSources'], type: 'inactive' });
}
