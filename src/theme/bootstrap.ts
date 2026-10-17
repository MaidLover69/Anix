// Imported first by main.tsx: puts the remembered theme (and system accent, when that option
// is on) on <html> before React renders, so the app does not start in the default theme and
// then change. The stored settings arrive later and have the last word (see useThemeSync).
import { applyTheme, storedTheme } from './themes';
import { applyAccentOverride, storedAccent } from './accent';

applyTheme(storedTheme());
const accent = storedAccent();
if (accent) applyAccentOverride(accent);
