/**
 * The built-in themes. Each one is a block of token overrides in `themes.css`, switched on
 * by `data-theme="<id>"` on `<html>`. The choice is the `theme` setting; a copy in
 * localStorage lets the next launch apply it before the settings have been read.
 */
export const THEMES = [
  { id: 'dark', name: 'Dark', scheme: 'dark' },
  { id: 'oled', name: 'OLED black', scheme: 'dark' },
  { id: 'light', name: 'Light', scheme: 'light' },
  { id: 'mocha', name: 'Catppuccin Mocha', scheme: 'dark' },
  { id: 'hacker', name: 'Hacker', scheme: 'dark' },
  { id: 'dracula', name: 'Dracula', scheme: 'dark' },
  { id: 'tokyo-night', name: 'Tokyo Night', scheme: 'dark' },
  { id: 'manga', name: 'Manga', scheme: 'light' },
  { id: 'comic', name: 'Comic', scheme: 'light' },
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
