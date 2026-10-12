// Imported first by main.tsx: puts the remembered theme on <html> before React renders, so
// the app does not start in the default theme and then change. The stored settings arrive
// later and have the last word (see useThemeSync).
import { applyTheme, storedTheme } from './themes';

applyTheme(storedTheme());
