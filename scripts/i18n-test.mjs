#!/usr/bin/env node
// Tests for the i18n rules, the setup "once per version" rule and the language-choice model.
//   node --experimental-strip-types scripts/i18n-test.mjs
import assert from 'node:assert/strict';
import {
  catalogProblems, htmlLang, interpolate, intlLocale, isRtl, resolveLanguage, translate,
} from '../src/i18n/core.ts';
import { isNewerVersion, needsSetup } from '../src/lib/version.ts';
import { withChoice } from '../src/lib/languageChoice.ts';

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

console.log('i18n tests passed');
