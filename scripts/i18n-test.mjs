#!/usr/bin/env node
// Tests for the i18n rules, the setup "once per version" rule and the language-choice model.
//   node --experimental-strip-types scripts/i18n-test.mjs
import assert from 'node:assert/strict';
import {
  APP_LANGUAGES, catalogProblems, htmlLang, interpolate, intlLocale, isRtl, resolveLanguage, translate,
} from '../src/i18n/core.ts';
import { isNewerVersion, needsSetup, shouldPromptUpdate } from '../src/lib/version.ts';
import { withChoice } from '../src/lib/languageChoice.ts';
import { errorCode, errorKeys } from '../src/lib/errorCodes.ts';
import fs from 'node:fs';

const en = {
  'a.hello': 'Hello, {name}!',
  'a.only': 'Only in English',
  'a.files': { one: '{count} file', other: '{count} files' },
};
const ru = {
  'a.hello': 'Привет, {name}!',
  'a.files': { one: '{count} файл', few: '{count} файла', many: '{count} файлов', other: '{count} файла' },
};
const rules = (code) => ({ catalog: new Intl.PluralRules(intlLocale(code)), fallback: new Intl.PluralRules('en') });

// Interpolation
assert.equal(translate(en, en, rules('en'), 'a.hello', { name: 'Mio' }), 'Hello, Mio!');
assert.equal(interpolate('{a} and {b}', { a: 1 }), '1 and {b}', 'unknown placeholders stay');
assert.equal(interpolate('{count}', { count: 0 }), '0');
// Plurals
assert.equal(translate(en, en, rules('en'), 'a.files', { count: 1 }), '1 file');
assert.equal(translate(en, en, rules('en'), 'a.files', { count: 2 }), '2 files');
assert.equal(translate(ru, en, rules('ru'), 'a.files', { count: 1 }), '1 файл');
assert.equal(translate(ru, en, rules('ru'), 'a.files', { count: 3 }), '3 файла');
assert.equal(translate(ru, en, rules('ru'), 'a.files', { count: 5 }), '5 файлов');
assert.equal(translate(ru, en, rules('ru'), 'a.files', { count: 21 }), '21 файл');
assert.equal(translate(ru, en, rules('ru'), 'a.files'), '{count} файла', 'no count: the "other" form');
// Fallback: missing key → English, unknown key → the key itself
assert.equal(translate(ru, en, rules('ru'), 'a.only'), 'Only in English');
assert.equal(translate(ru, en, rules('ru'), 'a.nothing'), 'a.nothing');
// A plural form the catalog lacks falls back to its own "other"
assert.equal(translate({ 'a.files': { other: '{count} x' } }, en, rules('fr'), 'a.files', { count: 1 }), '1 x');

// Language choice and direction
assert.equal(resolveLanguage('system', ['pt-BR', 'en-US']), 'pt');
assert.equal(resolveLanguage('system', ['zh_CN.UTF-8']), 'zh');
assert.equal(resolveLanguage(undefined, ['nl-NL', 'de-DE']), 'de');
assert.equal(resolveLanguage('system', ['nl-NL']), 'en');
assert.equal(resolveLanguage('ar', ['en-US']), 'ar', 'a saved language wins over the system');
assert.equal(resolveLanguage('xx', ['fr-FR']), 'fr', 'an unknown saved value follows the system');
assert.equal(isRtl('ar'), true);
assert.equal(isRtl('en'), false);
assert.equal(htmlLang('ar'), 'ar');
assert.equal(htmlLang('pt'), 'pt-BR');
assert.equal(new Intl.NumberFormat(intlLocale('ar')).format(1234), '1,234', 'Arabic keeps Latin digits');

// Catalog checker
assert.deepEqual(catalogProblems('ru', { ...ru, 'a.only': 'Только' }, en), []);
const bad = catalogProblems('ru', { 'a.hello': 'Привет!', 'a.files': { one: '{count}', other: '{count}' }, 'a.extra': 'x' }, en);
assert.ok(bad.some((p) => p.includes('missing "a.only"')));
assert.ok(bad.some((p) => p.includes('extra "a.extra"')));
assert.ok(bad.some((p) => p.includes('"a.hello" has placeholders')));
assert.ok(bad.some((p) => p.includes('"a.files" has plural forms')));

// Setup: first start, once after each upgrade, not again in the same version
assert.equal(needsSetup(undefined, '1.3.0'), true, 'first start');
assert.equal(needsSetup('', '1.3.0'), true);
assert.equal(needsSetup('1.2.0', '1.3.0'), true, 'after an upgrade');
assert.equal(needsSetup('1.3.0', '1.3.0'), false, 'already seen in this version');

// Update window: once per newer release, never over the player
assert.equal(shouldPromptUpdate('1.3.1', undefined, '/'), true, 'never shown before');
assert.equal(shouldPromptUpdate('1.3.1', '1.3.1', '/'), false, 'already shown for this release');
assert.equal(shouldPromptUpdate('1.3.2', '1.3.1', '/discover'), true, 'a newer release is shown again');
assert.equal(shouldPromptUpdate('1.3.1', '1.3.1 ', '/'), false, 'stray spaces in the saved version');
assert.equal(shouldPromptUpdate('1.3.1', undefined, '/watch/21/1'), false, 'not over the player');
assert.equal(shouldPromptUpdate(null, undefined, '/'), false, 'no update');
assert.equal(needsSetup('1.3.0', '1.3.1'), true);
assert.equal(needsSetup('1.4.0', '1.3.0'), false, 'a downgrade does not ask');
assert.equal(isNewerVersion('1.10.0', '1.9.9'), true);

// First/second/third choice
assert.deepEqual(withChoice(['en'], 1, 'es'), ['en', 'es']);
assert.deepEqual(withChoice(['en', 'es'], 2, 'fr'), ['en', 'es', 'fr']);
assert.deepEqual(withChoice(['en', 'es', 'fr'], 0, 'fr'), ['fr', 'es', 'en'], 'a code in another slot trades places');
assert.deepEqual(withChoice(['en', 'es', 'fr'], 1, ''), ['en', 'fr'], 'an emptied slot closes up');
assert.deepEqual(withChoice(['en', 'es'], 0, ''), ['en', 'es'], 'the first choice is never emptied');
assert.deepEqual(withChoice(['en', 'es', 'fr', 'it', 'de'], 2, 'it'), ['en', 'es', 'it', 'de'], 'older entries past the slots stay');
assert.deepEqual(withChoice(['en', 'es', 'fr', 'it'], 2, 'ja'), ['en', 'es', 'ja', 'it']);

// Error codes: every code errorCode can give has a translated text in every catalog, found
// through errorKeys (the code, its HTTP status class, then its area's general key).
const catalogs = Object.fromEntries(
  APP_LANGUAGES.map(({ code }) => [code, JSON.parse(fs.readFileSync(new URL(`../src/i18n/locales/${code}.json`, import.meta.url), 'utf8'))])
);
const prefixes = ['Network error: ', 'Connection failed: ', 'Parse error: ', 'Provider error: site — ', 'Other error: ', ''];
const subjects = ['AniList ', 'cdn.example '];
const problems = ['timed out', 'dns lookup failed', 'tls handshake', 'refused', 'is paused after failing a moment ago'];
for (let status = 100; status < 600; status++) problems.push(`HTTP ${status}`);
const messages = [
  "Blocked by the site's anti-bot check", 'site needs verification', 'No sources found',
  'Rate limited — retry after 5s', 'Auth required', 'Database error: disk full', 'Something a command wrote',
];
for (const prefix of prefixes) for (const subject of subjects) for (const problem of problems) messages.push(prefix + subject + problem);
const codes = new Set(messages.map(errorCode));
for (const code of ['CAT-403', 'CAT-ERR', 'CAT-PARSE', 'APP-ERR', 'NET-PARSE', 'SRC-ERR', 'SRC-502']) assert.ok(codes.has(code), code);
for (const message of messages) {
  if (errorCode(message) === 'APP-ERR' && !message.startsWith('Other error: ')) continue;
  const key = errorKeys(message).find((k) => k in catalogs.en);
  assert.ok(key, `no errors.* key for ${errorCode(message)} (${message})`);
  for (const [language, catalog] of Object.entries(catalogs)) assert.ok(key in catalog, `${language} lacks ${key}`);
}
assert.deepEqual(errorKeys('Network error: AniList HTTP 403: disabled'), ['errors.CAT-403', 'errors.CAT-4xx', 'errors.CAT-ERR']);
assert.deepEqual(errorKeys('Other error: MyAnimeList cannot find this anime'), ['errors.APP-ERR']);
assert.deepEqual(errorKeys('Something a command wrote'), [], 'a sentence written for the user stays as it is');
assert.deepEqual(errorKeys('Le texte déjà traduit'), [], 'text a screen already translated stays');
// A toast or inline line has no Details, so the generic "unexpected problem" must not replace
// the reason there (errorMessage); only ErrorState, which shows the raw text too, may use it.
const bell = 'Other error: Turn on the bell for this anime first';
assert.deepEqual(errorKeys(bell, false), [], 'without Details the reason stays');
const shownKey = errorKeys(bell, false).find((k) => k in catalogs.en);
assert.ok((shownKey ? catalogs.fr[shownKey] : bell).includes('Turn on the bell'), 'fr toast keeps the reason');
assert.deepEqual(errorKeys('Network error: AniList HTTP 403: disabled', false), ['errors.CAT-403', 'errors.CAT-4xx', 'errors.CAT-ERR']);

console.log('i18n tests passed');
