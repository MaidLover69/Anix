#!/usr/bin/env node
// Tests for the anime ids and episode numbers routes and links accept, the anime page's
// scores and the "not found" answer, Meme mode's level, rank and power level, how older settings
// load the Meme mode level, and the High line catalogue.
//   node --experimental-strip-types scripts/routes-test.mjs
import assert from 'node:assert/strict';
import { episodeExists, openAnimeId, parseAnimeId, parseDeepLink } from '../src/lib/routes.ts';
import { detailScores } from '../src/lib/scores.ts';
import { errorCode, isNotFoundError } from '../src/lib/errorCodes.ts';
import fs from 'node:fs';
import { expForLevel, playerLevel, powerLevel, rankFor } from '../src/lib/funLevel.ts';
import { isMemeLevel, memeLevelOf } from '../src/lib/memeLevel.ts';
import { ANIMATED, IMAGES, LINES, fillLine } from '../src/lib/memeCatalogue.ts';
import { readZip } from './memes-unpack.mjs';

// Ids: AniList ids and negative stand-ins; nothing else.
assert.equal(parseAnimeId('154587'), 154587);
assert.equal(parseAnimeId('-300000099'), -300000099);
for (const bad of ['0', '-0', '007', '1.5', '1e3', ' 1', 'abc', '', undefined, null, '3000000000', '-99999999999', '99999999999999999999']) {
  assert.equal(parseAnimeId(bad), null, String(bad));
}

// Deep links accept stand-in ids exactly like the routes.
assert.equal(parseDeepLink('anix://anime/21'), 21);
assert.equal(parseDeepLink('anix://anime/-300000099'), -300000099);
assert.equal(parseDeepLink('ANILILI://anime/21/?from=tray'), 21);
assert.equal(parseDeepLink('  anix://anime/21#x  '), 21);
for (const bad of ['anix://anime/0', 'anix://anime/--1', 'anix://anime/-', 'anix://anime/21/extra', 'anix://anime/99999999999999999999', 'https://anime/21', 'anix://watch/21']) {
  assert.equal(parseDeepLink(bad), null, bad);
}

// A clicked release alert or tray item opens stand-in anime too; 0 (the debug sample) does not.
assert.equal(openAnimeId(21), 21);
assert.equal(openAnimeId(-300000099), -300000099);
for (const bad of [0, 1.5, NaN, '21', null, undefined, 3e9]) assert.equal(openAnimeId(bad), null, String(bad));

// Episodes: whole numbers from 1; 0 and halves only when a list has them.
assert.equal(episodeExists(1), true);
assert.equal(episodeExists(9999), true, 'nothing known: any whole number from 1');
assert.equal(episodeExists(0), false);
assert.equal(episodeExists(12.5), false);
assert.equal(episodeExists(-1), false);
assert.equal(episodeExists(0, { listed: [0, 1, 2] }), true, 'a real episode 0');
assert.equal(episodeExists(12.5, { total: 24, listed: [12, 12.5, 13] }), true);
// A known count limits it.
assert.equal(episodeExists(24, { total: 24 }), true);
assert.equal(episodeExists(25, { total: 24 }), false);
assert.equal(episodeExists(9999, { total: 12 }), false);
assert.equal(episodeExists(13, { total: 12, listed: [13] }), true, 'an extra episode an extension lists');
assert.equal(episodeExists(26, { total: 12, listed: Array.from({ length: 26 }, (_, i) => i + 1) }), true);
// Airing without a total: the next-airing count may be stale, so it does not limit.
assert.equal(episodeExists(1160, { aired: 1150, airing: true }), true);
assert.equal(episodeExists(13, { total: 12, aired: 13, airing: true }), true);
// Finished, no total, but a list: the list limits.
assert.equal(episodeExists(30, { listed: [1, 2, 3] }), false);
assert.equal(episodeExists(3, { listed: [1, 2, 3] }), true);

// Scores: each from its real source, each source once.
assert.deepEqual(detailScores(85, undefined, 8.5), { percent: { source: 'AniList', score: 85 }, mal: 8.5 });
assert.deepEqual(detailScores(85, 'MyAnimeList', 8.5), { percent: null, mal: 8.5 }, 'the same MAL score once');
assert.deepEqual(detailScores(85, 'Jikan', 8.5), { percent: null, mal: 8.5 });
assert.deepEqual(detailScores(85, 'Shikimori', 8.5), { percent: null, mal: 8.5 }, "Shikimori's score is MAL's");
// MyAnimeList's score read through Jikan or Shikimori is credited to MyAnimeList.
assert.deepEqual(detailScores(85, 'Jikan', null), { percent: { source: 'MyAnimeList', score: 85 }, mal: null });
assert.deepEqual(detailScores(85, 'Shikimori', undefined), { percent: { source: 'MyAnimeList', score: 85 }, mal: null });
assert.deepEqual(detailScores(85, 'MyAnimeList', null), { percent: { source: 'MyAnimeList', score: 85 }, mal: null });
assert.deepEqual(detailScores(80, 'Kitsu', 8.5), { percent: { source: 'Kitsu', score: 80 }, mal: 8.5 });
assert.deepEqual(detailScores(null, 'Kitsu', 0), { percent: null, mal: null });

// "Not found" answers from any catalogue; a connection error is not one, and its code stays.
for (const m of [
  'Parse error: Anime with ID 999999 not found',
  'Parse error: MyAnimeList: anime 999998 not found',
  'Parse error: Kitsu: anime not found',
  'Parse error: IMDb: anime tt0000099 not found',
  'Parse error: AniList: not found (HTTP 404): Not Found.',
]) {
  assert.equal(isNotFoundError(m), true, m);
}
for (const m of ['Network error: AniList is paused after failing a moment ago', 'Network error: Comick HTTP 404: list not found', 'Parse error: AniList sent invalid JSON', null]) {
  assert.equal(isNotFoundError(m), false, String(m));
}
assert.equal(errorCode('Parse error: MyAnimeList: anime 999998 not found'), 'CAT-PARSE');

// Fun mode: level n takes 5·n² episodes in all; level 1 from nothing.
assert.equal(expForLevel(1), 0);
assert.equal(expForLevel(2), 20);
assert.equal(expForLevel(10), 500);
assert.deepEqual(playerLevel(0), { level: 1, rank: 'rookie', intoLevel: 0, levelSize: 20 });
assert.equal(playerLevel(19).level, 1);
assert.equal(playerLevel(20).level, 2, 'exactly at the boundary');
assert.deepEqual(playerLevel(50), { level: 3, rank: 'rookie', intoLevel: 5, levelSize: 35 });
assert.equal(playerLevel(125).level, 5);
for (const bad of [-3, NaN, Infinity]) assert.equal(playerLevel(bad).level, 1, String(bad));
for (let exp = 0; exp < 20000; exp += 7) {
  const { level, intoLevel, levelSize } = playerLevel(exp);
  assert.ok(intoLevel >= 0 && intoLevel < levelSize, `exp ${exp}`);
  assert.equal(expForLevel(level) + intoLevel, exp);
}
assert.deepEqual([1, 4, 5, 9, 10, 19, 20, 34, 35, 99].map(rankFor), [
  'rookie', 'rookie', 'otaku', 'otaku', 'senpai', 'senpai', 'sensei', 'sensei', 'legend', 'legend',
]);
// Power level: the 0–100 score first, then MyAnimeList's 0–10; 90 and up is "over 9000".
assert.deepEqual(powerLevel(85, null), { value: 8500, percent: 85, over9000: false });
assert.deepEqual(powerLevel(90, 7), { value: 9000, percent: 90, over9000: true });
assert.deepEqual(powerLevel(null, 9.12), { value: 9120, percent: 91.2, over9000: true });
assert.equal(powerLevel(0, 0), null);
assert.equal(powerLevel(null, undefined), null);

// Meme mode level from stored settings: a valid meme_mode wins, the old fun_mode: true is Medium,
// anything else (an unknown or null meme_mode too) is Low.
const levelCases = [
  [{}, 'low'],
  [{ fun_mode: true }, 'medium'],
  [{ fun_mode: false }, 'low'],
  [{ fun_mode: 'yes' }, 'low'],
  [{ meme_mode: 'high' }, 'high'],
  [{ meme_mode: 'medium' }, 'medium'],
  [{ meme_mode: 'high', fun_mode: false }, 'high'],
  [{ meme_mode: 'low', fun_mode: true }, 'low'],
  [{ meme_mode: 'max', fun_mode: true }, 'low'],
  [{ meme_mode: 3 }, 'low'],
  [{ meme_mode: null, fun_mode: true }, 'low'],
  [{ meme_mode: 'HIGH' }, 'low'],
];
for (const [stored, level] of levelCases) assert.equal(memeLevelOf(stored), level, JSON.stringify(stored));
assert.equal(isMemeLevel('medium'), true);
assert.equal(isMemeLevel('nope'), false);

// High's catalogue: every image is in the meme pack, animated ones have a separate still, lines
// stay short, parody lines are the credited ones, and nothing off-limits slips in.
const packManifest = () => {
  const unpacked = new URL('../public/memes/manifest.json', import.meta.url);
  if (fs.existsSync(unpacked)) return JSON.parse(fs.readFileSync(unpacked, 'utf8'));
  const entries = readZip(fs.readFileSync(new URL('../assets/memes.zip', import.meta.url)));
  return JSON.parse(entries.find((e) => e.name === 'memes/manifest.json').data.toString('utf8'));
};
const manifest = packManifest();
assert.deepEqual([...IMAGES], manifest.memes.map((m) => m.id).sort());
for (const id of ANIMATED) {
  const meme = manifest.memes.find((m) => m.id === id);
  assert.ok(meme && meme.still !== meme.file, `${id} has a still frame`);
}
const UBW_LINES = new Set([
  'Life’s a crazy bag of marbles.', 'Well now what do we do?!', 'Was history just anime?', 'I can wait.', 'Not you too!',
  'Is that something people say now?', 'THIS IS THE WORST TIMELINE!', 'Heckin’… I dunno, man.',
]);
for (const [spot, entry] of Object.entries(LINES)) {
  if (entry.img) assert.ok(IMAGES.includes(entry.img), `${spot}: ${entry.img}`);
  if (entry.plate) assert.ok(entry.img, `${spot}: a plate needs an image`);
  assert.equal(entry.credit === 'ubw', UBW_LINES.has(entry.line), `${spot}: parody credit`);
  for (const line of [entry.line, entry.lineRtl].filter(Boolean)) {
    assert.ok(line.length <= 80, `${spot} is short`);
    assert.doesNotMatch(line, /ara ara|dekai|yamete|1000-7|nice boat/i, spot);
  }
}
// Every pack image is shown somewhere: a spot's own image or a <MemeImage name="…"> in the app.
const sources = fs
  .readdirSync(new URL('../src', import.meta.url), { recursive: true })
  .filter((file) => file.endsWith('.tsx'))
  .map((file) => fs.readFileSync(new URL(`../src/${file}`, import.meta.url), 'utf8'))
  .join('\n');
const spotImages = new Set(Object.values(LINES).map((entry) => entry.img));
for (const id of IMAGES) assert.ok(spotImages.has(id) || sources.includes(`name="${id}"`), `${id} is never shown`);
assert.equal(fillLine('S-rank {sc}%', { sc: 91 }), 'S-rank 91%');
assert.equal(fillLine('Now {t} ← you are here', {}), 'Now  ← you are here');
assert.equal(fillLine('第{n}話 {x}', { n: 3 }), '第3話 ');

console.log('routes tests passed');
