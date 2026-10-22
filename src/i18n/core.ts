/**
 * The i18n rules with no app or bundler imports, so node scripts (scripts/i18n-*.mjs) can load
 * this file directly. `index.ts` holds the running state and the React side.
 */

/** A catalog value: text, or one text per plural category ("one", "few", "other"…). */
export type Entry = string | Partial<Record<Intl.LDMLPluralRule, string>>;
export type Catalog = Record<string, Entry>;
export type Vars = Record<string, string | number>;

/** Interface languages, English first (the source). `name` is the language's own name. */
export const APP_LANGUAGES = [
  { code: 'en', name: 'English' },
  { code: 'fr', name: 'Français' },
  { code: 'ja', name: '日本語' },
  { code: 'ko', name: '한국어' },
  { code: 'zh', name: '简体中文' },
  { code: 'pt', name: 'Português (Brasil)' },
  { code: 'ru', name: 'Русский' },
  { code: 'hi', name: 'हिन्दी' },
  { code: 'es', name: 'Español' },
  { code: 'de', name: 'Deutsch' },
  { code: 'ar', name: 'العربية' },
] as const;

export type AppLanguage = (typeof APP_LANGUAGES)[number]['code'];

/** The `app_language` setting value that follows the system. */
export const SYSTEM_LANGUAGE = 'system';

export function isAppLanguage(code: unknown): code is AppLanguage {
  return APP_LANGUAGES.some((language) => language.code === code);
}

export function isRtl(code: string): boolean {
  return code === 'ar';
}

/** The BCP 47 tag for `lang=` and `Intl`: Brazilian Portuguese, Simplified Chinese, Latin digits in Arabic. */
export function intlLocale(code: string): string {
  switch (code) {
    case 'zh':
      return 'zh-CN';
    case 'pt':
      return 'pt-BR';
    case 'ar':
      // Times, episode numbers and the seek bar read the same as on the player.
      return 'ar-u-nu-latn';
    default:
      return code;
  }
}

/** The tag for the `lang` attribute (no Unicode extensions). */
export function htmlLang(code: string): string {
  return intlLocale(code).split('-u-')[0];
}

/**
 * The interface language for an `app_language` setting: a supported code as saved, otherwise
 * ("system", missing, unknown) the first system language Anix has, else English.
 */
export function resolveLanguage(setting: unknown, systemLanguages: readonly string[]): AppLanguage {
  if (isAppLanguage(setting)) return setting;
  for (const tag of systemLanguages) {
    const primary = tag.trim().toLowerCase().split(/[-_]/)[0];
    if (isAppLanguage(primary)) return primary;
  }
  return 'en';
}

/** Replaces `{name}` with `vars.name`; unknown placeholders stay as written. */
export function interpolate(text: string, vars?: Vars): string {
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    Object.prototype.hasOwnProperty.call(vars, name) ? String(vars[name]) : whole
  );
}

/** The placeholder names in a text, sorted. */
export function placeholders(text: string): string[] {
  return [...new Set([...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]))].sort();
}

/** One form of an entry: plural entries pick by `count` (else "other"). */
export function pickForm(entry: Entry, count: number | undefined, rules: Intl.PluralRules): string | undefined {
  if (typeof entry === 'string') return entry;
  if (typeof count === 'number') {
    const form = entry[rules.select(count)];
    if (form !== undefined) return form;
  }
  return entry.other;
}

/**
 * The text for `key`: from `catalog` in its language, else from the English `fallback`, else the
 * key itself (so a missing key shows up instead of an empty label). `vars.count` picks the plural form.
 */
export function translate(
  catalog: Catalog,
  fallback: Catalog,
  rules: { catalog: Intl.PluralRules; fallback: Intl.PluralRules },
  key: string,
  vars?: Vars
): string {
  const count = typeof vars?.count === 'number' ? vars.count : undefined;
  const own = catalog[key];
  const text =
    (own !== undefined ? pickForm(own, count, rules.catalog) : undefined) ??
    (fallback[key] !== undefined ? pickForm(fallback[key], count, rules.fallback) : undefined) ??
    key;
  return interpolate(text, vars);
}

/**
 * What is wrong with a catalog compared to English: missing and extra keys, plural entries
 * without the forms the language needs, and placeholders that differ. Empty when it is fine.
 */
export function catalogProblems(code: string, catalog: Catalog, english: Catalog): string[] {
  const problems: string[] = [];
  const needed = new Intl.PluralRules(intlLocale(code)).resolvedOptions().pluralCategories as string[];
  const formsOf = (entry: Entry) => (typeof entry === 'string' ? [entry] : Object.values(entry));
  const names = (entry: Entry) => [...new Set(formsOf(entry).flatMap(placeholders))].sort().join(',');
  for (const key of Object.keys(english)) {
    if (!(key in catalog)) problems.push(`${code}: missing "${key}"`);
  }
  for (const [key, entry] of Object.entries(catalog)) {
    const source = english[key];
    if (source === undefined) {
      problems.push(`${code}: extra "${key}"`);
      continue;
    }
    if (typeof source === 'string' ? typeof entry !== 'string' : typeof entry !== 'object' || entry === null) {
      problems.push(`${code}: "${key}" must be ${typeof source === 'string' ? 'text' : 'plural forms'}`);
      continue;
    }
    if (typeof entry === 'object') {
      const forms = Object.keys(entry).sort();
      const want = [...needed].sort();
      if (forms.join(',') !== want.join(',')) {
        problems.push(`${code}: "${key}" has plural forms [${forms}] but ${code} needs [${want}]`);
      }
    }
    if (formsOf(entry).some((text) => typeof text !== 'string' || text.trim() === '')) {
      problems.push(`${code}: "${key}" is empty`);
    }
    if (names(entry) !== names(source)) {
      problems.push(`${code}: "${key}" has placeholders {${names(entry)}} but English has {${names(source)}}`);
    }
  }
  return problems;
}
