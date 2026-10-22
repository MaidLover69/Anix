import { useEffect } from 'react';
import { useSettings, useSaveSettings } from './useSettings';
import { useDesktopAppearance } from './useDesktopAppearance';
import { applyLook, applyTheme, currentTheme, isThemeId, DEFAULT_THEME } from '../theme/themes';
import { applyAccentOverride, parseHexColor } from '../theme/accent';
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

  // The accent is derived per theme, so it is worked out again on every theme change. The
  // system accent, while that option is on, wins over a colour picked in Settings.
  const useSystemAccent = settings?.use_system_accent === true;
  const desktop = useDesktopAppearance();
  const customAccent = parseHexColor(settings?.accent_color) ? settings?.accent_color ?? null : null;
  const accent = useSystemAccent ? desktop.data?.accent_color ?? null : customAccent;
  const accentKnown = settings !== undefined && (!useSystemAccent || !desktop.isPending);
  useEffect(() => {
    if (!accentKnown) return;
    applyAccentOverride(accent);
  }, [accentKnown, accent, saved]);

  const loaded = settings !== undefined;
  const contrast = settings?.contrast;
  const uiSize = settings?.ui_size;
  const art = settings?.background_art;
  const artUrl = art?.url;
  const artStyle = art?.style;
  useEffect(() => {
    if (!loaded) return;
    applyLook({ contrast, uiSize, art: artUrl ? { url: artUrl, style: artStyle === 'plain' ? 'plain' : 'blur' } : null });
  }, [loaded, contrast, uiSize, artUrl, artStyle]);
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
