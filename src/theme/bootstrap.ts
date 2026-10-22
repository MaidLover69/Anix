// Imported first by main.tsx: puts the remembered theme, look (contrast, size, art) and accent on <html> before React renders, so the app does not start in the default theme and
// then change. The stored settings arrive later and have the last word (see useThemeSync).
import { applyLook, applyTheme, storedLook, storedTheme } from './themes';
import { applyAccentOverride, storedAccent } from './accent';

applyTheme(storedTheme());
applyLook(storedLook());
const accent = storedAccent();
if (accent) applyAccentOverride(accent);
