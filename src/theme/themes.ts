/**
 * The built-in themes. Each one is a block of token overrides in `themes.css`, switched on
 * by `data-theme="<id>"` on `<html>`. The choice is the `theme` setting; a copy in
 * localStorage lets the next launch apply it before the settings have been read.
 * Names that are words are i18n keys (`nameKey`); proper names stay as they are.
 */
export const THEMES = [
  { id: 'dark', nameKey: 'themes.dark', scheme: 'dark' },
  { id: 'oled', nameKey: 'themes.oled', scheme: 'dark' },
  { id: 'light', nameKey: 'themes.light', scheme: 'light' },
  { id: 'mocha', name: 'Catppuccin Mocha', scheme: 'dark' },
  { id: 'hacker', nameKey: 'themes.hacker', scheme: 'dark' },
  { id: 'dracula', name: 'Dracula', scheme: 'dark' },
  { id: 'tokyo-night', name: 'Tokyo Night', scheme: 'dark' },
  { id: 'manga', nameKey: 'themes.manga', scheme: 'light' },
  { id: 'comic', nameKey: 'themes.comic', scheme: 'light' },
] as const;

export type ThemeId = (typeof THEMES)[number]['id'];

export const DEFAULT_THEME: ThemeId = 'dark';

const THEME_STORAGE_KEY = 'anix-theme';

export function isThemeId(value: unknown): value is ThemeId {
  return THEMES.some((theme) => theme.id === value);
}

/** The theme remembered from the last run, or the default. */
export function storedTheme(): ThemeId {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    return isThemeId(stored) ? stored : DEFAULT_THEME;
  } catch {
    return DEFAULT_THEME;
  }
}

/** The theme the page is showing right now. */
export function currentTheme(): ThemeId {
  const active = document.documentElement.dataset.theme;
  return isThemeId(active) ? active : DEFAULT_THEME;
}

/** Switches the page to `theme` and remembers it for the next launch. */
export function applyTheme(theme: ThemeId): void {
  const root = document.documentElement;
  root.dataset.theme = theme;
  // index.html paints <html> dark inline so the window never opens white. From here on the
  // stylesheet sets the background per theme, and the inline value would win over it.
  root.style.removeProperty('background-color');
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Storage can be unavailable; the theme still applies for this run.
  }
}

/**
 * The look settings that sit on top of any theme (Settings › Appearance): contrast level,
 * interface size and background art, as `data-contrast`, `data-ui-size` and `data-art` on
 * <html> (rules in themes.css). Remembered like the theme so the next launch starts with them.
 */
export interface Look {
  contrast?: 'standard' | 'medium' | 'high';
  uiSize?: 'small' | 'medium' | 'large';
  art?: { url: string; style: 'blur' | 'plain' } | null;
}

const LOOK_STORAGE_KEY = 'anix-look';

export function applyLook(look: Look): void {
  const root = document.documentElement;
  const set = (name: string, value: string | undefined) => {
    if (value) root.setAttribute(name, value);
    else root.removeAttribute(name);
  };
  set('data-contrast', look.contrast && look.contrast !== 'standard' ? look.contrast : undefined);
  set('data-ui-size', look.uiSize && look.uiSize !== 'medium' ? look.uiSize : undefined);
  const art = look.art && /^https?:\/\//.test(look.art.url) ? look.art : null;
  set('data-art', art ? art.style : undefined);
  if (art) root.style.setProperty('--app-art', `url(${JSON.stringify(art.url)})`);
  else root.style.removeProperty('--app-art');
  try {
    localStorage.setItem(LOOK_STORAGE_KEY, JSON.stringify({ ...look, art }));
  } catch {
    // Storage can be unavailable; the look still applies for this run.
  }
}

/** The look remembered from the last run. */
export function storedLook(): Look {
  try {
    const stored = JSON.parse(localStorage.getItem(LOOK_STORAGE_KEY) ?? '{}');
    return stored && typeof stored === 'object' ? stored : {};
  } catch {
    return {};
  }
}
