#!/usr/bin/env node
// Checks the interface translations. Run from the app folder:
//   node --experimental-strip-types scripts/i18n-check.mjs          catalogs only
//   node --experimental-strip-types scripts/i18n-check.mjs --raw    also untranslated text in src/
// Fails (exit 1) when a catalog misses keys, has extra keys, lacks plural forms its language
// needs or changes placeholders; with --raw, also when screens or components still hold text
// written straight into JSX or into aria-label/title/placeholder/alt/label/hint attributes.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { APP_LANGUAGES, catalogProblems } from '../src/i18n/core.ts';

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dir = path.join(app, 'src/i18n/locales');
const read = (code) => JSON.parse(fs.readFileSync(path.join(dir, `${code}.json`), 'utf8'));

const problems = [];
const english = read('en');
const counts = [];
for (const { code } of APP_LANGUAGES) {
  if (code === 'en') continue;
  if (!fs.existsSync(path.join(dir, `${code}.json`))) {
    problems.push(`${code}: no catalog (src/i18n/locales/${code}.json)`);
    continue;
  }
  let catalog;
  try {
    catalog = read(code);
  } catch (err) {
    problems.push(`${code}: not valid JSON (${err.message})`);
    continue;
  }
  const found = catalogProblems(code, catalog, english);
  problems.push(...found);
  const same = Object.keys(english).filter((key) => JSON.stringify(catalog[key]) === JSON.stringify(english[key]));
  counts.push(`${code} ${Object.keys(catalog).length} keys (${same.length} same as English)`);
}
for (const [key, value] of Object.entries(english)) {
  const forms = typeof value === 'string' ? [value] : Object.values(value);
  if (typeof value !== 'string' && !('one' in value && 'other' in value)) problems.push(`en: "${key}" needs "one" and "other"`);
  if (forms.some((text) => typeof text !== 'string' || text.trim() === '')) problems.push(`en: "${key}" is empty`);
}

// Text in JSX or in naming attributes that did not go through t(). Allowed: product names,
// key names and symbols, which read the same in every language.
const ALLOWED = new Set([
  'Anix', 'AniList', 'MyAnimeList', 'MAL', 'Comick', 'mpv', 'VLC', 'IMDb', 'Kitsu', 'Shikimori', 'Jikan',
  'AnimeThemes', 'AniSkip', 'MPRIS', 'VA-API', 'HLS', 'Ctrl', 'Shift', 'Alt', 'Esc', 'Space', 'Enter', 'Tab',
  'OP', 'ED', 'HD', 'SD', 'GPU', 'URL', 'JSON', 'CSV', 'DoH', 'Anime4K', 'AppImage', 'GitHub', 'Seanime',
]);
const ATTRS = /\b(aria-label|title|placeholder|alt|label|hint|description|tooltip)="([^"]*[A-Za-z]{2,}[^"]*)"/g;
const JSX_TEXT = />\s*([^<>{}`]*[A-Za-z]{2,}[^<>{}`]*?)\s*</g;
const raw = [];
function scan(file) {
  const rel = path.relative(app, file);
  const lines = fs.readFileSync(file, 'utf8').split('\n');
  lines.forEach((line, i) => {
    if (/^\s*(\/\/|\*|\/\*)/.test(line) || /i18n-ignore/.test(line)) return;
    const hits = [];
    for (const m of line.matchAll(ATTRS)) hits.push(m[2]);
    for (const m of line.matchAll(JSX_TEXT)) {
      // `a < b > c` in code, generics and arrows (`=> invoke<T>`) are not JSX text.
      if (/[=;()]|=>|&&|\|\|/.test(m[1]) || line[m.index - 1] === '=') continue;
      hits.push(m[1]);
    }
    // A line of plain words between JSX tags (multi-line text).
    const bare = /^\s+([A-Za-z][A-Za-z0-9 ,.'’!?…:;()\-–—/]*[A-Za-z.!?…)])\s*$/.exec(line);
    if (bare && /\s/.test(bare[1]) && !/^(import|export|return|const|let|if|else|case|type|interface|from)\b/.test(bare[1])
      && !/[;{}=]$/.test(line.trim()) && /<\/?[\w.]+[^>]*>?\s*$/.test(lines[i - 1] ?? '')) hits.push(bare[1]);
    for (const text of hits) {
      const words = text.trim();
      if (!words || ALLOWED.has(words) || !/[A-Za-z]{2,}/.test(words)) continue;
      if (words.split(/[\s·/+,]+/).every((w) => ALLOWED.has(w) || !/[A-Za-z]{2,}/.test(w))) continue;
      raw.push(`${rel}:${i + 1}: ${words}`);
    }
  });
}
function walk(folder) {
  for (const name of fs.readdirSync(folder)) {
    const full = path.join(folder, name);
    if (fs.statSync(full).isDirectory()) walk(full);
    else if (name.endsWith('.tsx')) scan(full);
  }
}
if (process.argv.includes('--raw')) {
  for (const folder of ['src/screens', 'src/components']) walk(path.join(app, folder));
  for (const file of ['src/router.tsx', 'src/App.tsx']) scan(path.join(app, file));
}

console.log(`en ${Object.keys(english).length} keys (source)`);
for (const line of counts) console.log(line);
for (const line of raw) console.log(`raw text: ${line}`);
for (const line of problems) console.log(`problem: ${line}`);
if (problems.length || raw.length) {
  console.log(`\n${problems.length} catalog problem(s), ${raw.length} untranslated text(s)`);
  process.exit(1);
}
console.log('\nAll catalogs match English.');
