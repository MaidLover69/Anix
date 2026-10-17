/**
 * "Use system accent colour": the desktop's accent replaces the accent ramp of the current
 * theme, as inline properties on <html> (they win over the theme blocks). The ramp is derived
 * per theme so accent text (accent-300, its hover accent-200) keeps a contrast of at least
 * 4.5:1 on the theme's surfaces, and text on filled accent (on-accent over accent-600) too.
 * Manga is black and white by design and keeps its own ramp; Comic keeps its yellow tint.
 */
import { currentTheme, THEMES } from './themes';
import type { ThemeId } from './themes';

type RGB = [number, number, number];

const ACCENT_STORAGE_KEY = 'anix-system-accent';
const MIN_CONTRAST = 4.5;
const THEMES_WITHOUT_OVERRIDE: ThemeId[] = ['manga'];

const OVERRIDDEN = [
  '--accent-950',
  '--accent-900',
  '--accent-700',
  '--accent-600',
  '--accent-500',
  '--accent-400',
  '--accent-300',
  '--accent-200',
  '--accent-100',
  '--accent-tint',
  '--on-accent',
  '--border-focus',
] as const;

export function parseHexColor(hex: string | null | undefined): RGB | null {
  const match = hex?.trim().match(/^#?([0-9a-f]{6})$/i);
  if (!match) return null;
  const value = parseInt(match[1], 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

function parseChannels(value: string): RGB | null {
  const parts = value.trim().split(/[\s,]+/).map(Number);
  return parts.length >= 3 && parts.slice(0, 3).every(Number.isFinite) ? [parts[0], parts[1], parts[2]] : null;
}

function luminance([r, g, b]: RGB): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

export function contrastRatio(a: RGB, b: RGB): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

function rgbToHsl([r, g, b]: RGB): [number, number, number] {
  const [rn, gn, bn] = [r / 255, g / 255, b / 255];
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === rn) h = (gn - bn) / d + (gn < bn ? 6 : 0);
  else if (max === gn) h = (bn - rn) / d + 2;
  else h = (rn - gn) / d + 4;
  return [h / 6, s, l];
}

function hslToRgb(h: number, s: number, l: number): RGB {
  const clamp = (v: number) => Math.min(1, Math.max(0, v));
  const [sc, lc] = [clamp(s), clamp(l)];
  if (sc === 0) {
    const v = Math.round(lc * 255);
    return [v, v, v];
  }
  const q = lc < 0.5 ? lc * (1 + sc) : lc + sc - lc * sc;
  const p = 2 * lc - q;
  const hue = (t: number) => {
    let x = t;
    if (x < 0) x += 1;
    if (x > 1) x -= 1;
    if (x < 1 / 6) return p + (q - p) * 6 * x;
    if (x < 1 / 2) return q;
    if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
    return p;
  };
  return [hue(h + 1 / 3), hue(h), hue(h - 1 / 3)].map((v) => Math.round(v * 255)) as RGB;
}

function mix(a: RGB, b: RGB, amount: number): RGB {
  return a.map((v, i) => Math.round(v + (b[i] - v) * amount)) as RGB;
}

const channels = (c: RGB) => c.join(' ');

/**
 * The lightness, moved from `l` in `step` direction, at which the colour reaches
 * `MIN_CONTRAST` against every one of `against`.
 */
function readableLightness(h: number, s: number, l: number, step: number, against: RGB[]): number {
  let value = l;
  for (let i = 0; i < 120; i++) {
    const rgb = hslToRgb(h, s, value);
    if (against.every((bg) => contrastRatio(rgb, bg) >= MIN_CONTRAST)) return value;
    const next = value + step;
    if (next > 1 || next < 0) break;
    value = next;
  }
  return step > 0 ? 1 : 0;
}

const WHITE: RGB = [255, 255, 255];
const BLACK: RGB = [12, 12, 16];

/**
 * The accent tokens for `accent` on a theme with the given `scheme` and `surfaces` (the
 * backgrounds accent text sits on). Values are RGB channels except the tint and focus ring.
 */
export function deriveAccentTokens(
  accent: RGB,
  scheme: 'dark' | 'light',
  surfaces: RGB[],
  options: { keepTint?: boolean } = {}
): Record<string, string> {
  const [h, s, l] = rgbToHsl(accent);
  const dark = scheme === 'dark';
  // Text sits on the plain surfaces and on the accent tint laid over them.
  const textGrounds = [...surfaces, ...surfaces.map((bg) => mix(bg, accent, 0.16))];
  const textStep = dark ? 0.01 : -0.01;
  const textL = readableLightness(h, s, l, textStep, textGrounds);
  const text = hslToRgb(h, s, textL);
  const hover = hslToRgb(h, s, readableLightness(h, s, textL + (dark ? 0.08 : -0.08), textStep, textGrounds));
  const strongest = hslToRgb(h, s, readableLightness(h, s, textL + (dark ? 0.16 : -0.16), textStep, textGrounds));

  // The filled button: the accent itself, with whichever of white or ink reads better on it;
  // moved away from that text colour until the pair reaches 4.5:1.
  const whiteOnAccent = contrastRatio(WHITE, accent) >= contrastRatio(BLACK, accent);
  const onAccent = whiteOnAccent ? WHITE : BLACK;
  let fillL = l;
  for (let i = 0; i < 100 && contrastRatio(onAccent, hslToRgb(h, s, fillL)) < MIN_CONTRAST; i++) {
    fillL += whiteOnAccent ? -0.01 : 0.01;
  }
  const fill = hslToRgb(h, s, fillL);
  const fillHover = hslToRgb(h, s, fillL + (whiteOnAccent ? -0.06 : 0.05));
  const base = accent;
  const mid = mix(base, text, 0.5);

  const tokens: Record<string, string> = {
    '--accent-950': channels(hslToRgb(h, Math.min(s, 0.6), dark ? 0.14 : 0.96)),
    '--accent-900': channels(hslToRgb(h, Math.min(s, 0.65), dark ? 0.26 : 0.9)),
    '--accent-700': channels(fillHover),
    '--accent-600': channels(fill),
    '--accent-500': channels(base),
    '--accent-400': channels(mid),
    '--accent-300': channels(text),
    '--accent-200': channels(hover),
    '--accent-100': channels(strongest),
    '--on-accent': channels(onAccent),
    '--border-focus': `rgb(${channels(base)} / 0.5)`,
  };
  if (!options.keepTint) tokens['--accent-tint'] = `rgb(${channels(base)} / ${dark ? 0.14 : 0.12})`;
  return tokens;
}

function clearOverride(root: HTMLElement) {
  for (const name of OVERRIDDEN) root.style.removeProperty(name);
}

/**
 * Applies `hex` as the accent of the theme on screen, or takes the override off (null).
 * Remembers it so the next launch starts with it (see bootstrap.ts).
 */
export function applyAccentOverride(hex: string | null): void {
  const root = document.documentElement;
  clearOverride(root);
  const accent = parseHexColor(hex);
  try {
    if (accent && hex) localStorage.setItem(ACCENT_STORAGE_KEY, hex);
    else localStorage.removeItem(ACCENT_STORAGE_KEY);
  } catch {
    // Storage can be unavailable; the override still applies for this run.
  }
  const theme = currentTheme();
  if (!accent || THEMES_WITHOUT_OVERRIDE.includes(theme)) return;

  // Surfaces of the theme itself, read after the override was cleared.
  const style = getComputedStyle(root);
  const surfaces = ['--bg-page', '--bg-card', '--bg-elevated', '--bg-surface']
    .map((name) => parseChannels(style.getPropertyValue(name)))
    .filter((c): c is RGB => c !== null);
  const scheme = THEMES.find((t) => t.id === theme)?.scheme ?? 'dark';
  const tokens = deriveAccentTokens(accent, scheme, surfaces.length > 0 ? surfaces : [[10, 10, 13]], {
    keepTint: theme === 'comic',
  });
  for (const [name, value] of Object.entries(tokens)) root.style.setProperty(name, value);
}

/** The accent remembered from the last run, if the option was on. */
export function storedAccent(): string | null {
  try {
    return localStorage.getItem(ACCENT_STORAGE_KEY);
  } catch {
    return null;
  }
}
