/**
 * Interface text. `t('key', { name })` gives the text in the active language, falling back to
 * English (`locales/en.json`, the source) for anything a catalog lacks. Catalogs other than
 * English load on demand. Changing the language re-mounts the app (App.tsx keys it on
 * `useLocale()`), so `t()` may be called anywhere at render time — but never at module load,
 * where it would keep the language of the first import.
 */
/// <reference types="vite/client" />
import React, { useSyncExternalStore } from 'react';
import en from './locales/en.json';
import {
  type AppLanguage,
  type Catalog,
  type Vars,
  htmlLang,
  intlLocale,
  isAppLanguage,
  isRtl,
  resolveLanguage,
  translate,
} from './core';

export { APP_LANGUAGES, SYSTEM_LANGUAGE, isAppLanguage, resolveLanguage, type AppLanguage } from './core';

export type TKey = keyof typeof en & string;

const english = en as Catalog;
const loaders = import.meta.glob<{ default: Catalog }>(['./locales/*.json', '!./locales/en.json']);

/** Remembers the `app_language` setting so the next start renders in it from the first frame. */
const STORAGE_KEY = 'anix-app-language';

let current: AppLanguage = 'en';
let catalog: Catalog = english;
let rules = { catalog: new Intl.PluralRules('en'), fallback: new Intl.PluralRules('en') };
const listeners = new Set<() => void>();

export function t(key: TKey, vars?: Vars): string {
  return translate(catalog, english, rules, key, vars);
}

/** True for a key the English catalog has (for keys built at run time, such as error codes). */
export function isKey(key: string): key is TKey {
  return Object.prototype.hasOwnProperty.call(english, key);
}

/** The English source text, for notes shown in English next to the translation. */
export function englishText(key: TKey, vars?: Vars): string {
  return translate(english, english, { catalog: rules.fallback, fallback: rules.fallback }, key, vars);
}

/**
 * Like `t`, for text with elements inside: `tn('x.pressKey', { key: <kbd>K</kbd> })` with
 * "Press {key} to search" keeps the word order of each language.
 */
export function tn(key: TKey, nodes: Record<string, React.ReactNode>, vars?: Vars): React.ReactNode {
  const text = t(key, vars);
  const parts = text.split(/\{(\w+)\}/g);
  return parts.map((part, i) =>
    i % 2 === 1 && part in nodes
      ? React.createElement(React.Fragment, { key: i }, nodes[part])
      : i % 2 === 1
        ? `{${part}}`
        : part
  );
}

/** The active interface language code ("en", "ar"…). */
export function language(): AppLanguage {
  return current;
}

/** The tag for `Intl` and `toLocale…String` calls, so dates and numbers follow the interface language. */
export function locale(): string {
  return intlLocale(current);
}

export function formatNumber(value: number, options?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(locale(), options).format(value);
}

/** Re-renders on a language change. */
export function useLocale(): AppLanguage {
  return useSyncExternalStore(
    (onChange) => {
      listeners.add(onChange);
      return () => listeners.delete(onChange);
    },
    () => current
  );
}

function systemLanguages(): readonly string[] {
  return typeof navigator === 'undefined' ? [] : navigator.languages ?? [navigator.language];
}

function rememberSetting(setting: string) {
  try {
    localStorage.setItem(STORAGE_KEY, setting);
  } catch {
    // Storage may be unavailable; the stored settings still decide once they load.
  }
}

function rememberedSetting(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

/** Loads a language's catalog and makes it active; the page direction follows (Arabic is right to left). */
let loading = 0;
/** The language last asked for: active, or still loading. */
let target: AppLanguage | null = null;

export async function setLanguage(code: AppLanguage): Promise<void> {
  const ticket = ++loading;
  target = code;
  let next = english;
  if (code !== 'en') {
    const load = loaders[`./locales/${code}.json`];
    try {
      next = load ? (await load()).default : english;
    } catch (err) {
      console.warn(`[i18n] could not load the ${code} catalog; English is shown`, err);
    }
  }
  // A later choice was made while this catalog loaded.
  if (ticket !== loading) return;
  current = code;
  catalog = next;
  rules = { catalog: new Intl.PluralRules(intlLocale(code)), fallback: rules.fallback };
  const root = document.documentElement;
  root.lang = htmlLang(code);
  root.dir = isRtl(code) ? 'rtl' : 'ltr';
  listeners.forEach((notify) => notify());
}

/** Applies an `app_language` setting ('system', a code, or nothing) and remembers it for the next start. */
export function applyLanguageSetting(setting: unknown): Promise<void> {
  const value = isAppLanguage(setting) ? setting : 'system';
  rememberSetting(value);
  const code = resolveLanguage(value, systemLanguages());
  return code === target ? Promise.resolve() : setLanguage(code);
}

/** The language "Use system language" stands for right now. */
export function systemLanguage(): AppLanguage {
  return resolveLanguage('system', systemLanguages());
}

/** Before the first render: the language remembered from the last run. */
export function initI18n(): Promise<void> {
  return setLanguage(resolveLanguage(rememberedSetting() ?? 'system', systemLanguages()));
}
