import { useEffect } from 'react';
import { useSettings, useSaveSettings } from './useSettings';
import { useDesktopAppearance } from './useDesktopAppearance';
import { applyTheme, currentTheme, isThemeId, DEFAULT_THEME } from '../theme/themes';
import { applyAccentOverride } from '../theme/accent';
import type { ThemeId } from '../theme/themes';

/**
 * Keeps the page on the theme in the stored settings. The bootstrap has already applied the
 * copy remembered in localStorage; this corrects it if the settings file says otherwise.
 */
export function useThemeSync(): void {
  const { data: settings } = useSettings();
  const saved = settings?.theme;

  useEffect(() => {
    if (saved === undefined) return;
    const theme = isThemeId(saved) ? saved : DEFAULT_THEME;
    if (theme !== currentTheme()) applyTheme(theme);
  }, [saved]);

  // The system accent is derived per theme, so it is worked out again on every theme change.
  const useSystemAccent = settings?.use_system_accent === true;
  const desktop = useDesktopAppearance();
  const systemAccent = useSystemAccent ? desktop.data?.accent_color ?? null : null;
  const accentKnown = settings !== undefined && (!useSystemAccent || !desktop.isPending);
  useEffect(() => {
    if (!accentKnown) return;
    applyAccentOverride(systemAccent);
  }, [accentKnown, systemAccent, saved]);
}

/** The selected theme and a setter that applies it at once and saves it. */
export function useTheme(): { theme: ThemeId; setTheme: (theme: ThemeId, onError?: (error: unknown) => void) => void } {
  const { data: settings } = useSettings();
  const save = useSaveSettings();
  const theme = isThemeId(settings?.theme) ? settings.theme : currentTheme();

  const setTheme = (next: ThemeId, onError?: (error: unknown) => void) => {
    applyTheme(next);
    save.mutate({ theme: next }, { onError });
  };

  return { theme, setTheme };
}
