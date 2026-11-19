#!/usr/bin/env node
// Issue #6 (rest of collections), RED tests for the TypeScript side. They fail until built.
//   node --experimental-strip-types scripts/collections-test.mjs
// The Rust side is src-tauri/tests/collections_v142_tests.rs and the studio cases of
// catalogue_tests.rs; the expected API of both is src-tauri/tests/collections_v142_API.md.
//
// Seams (all pure, no React imports):
//   smartRule.ts   studio rule; ruleForFranchise(id) / franchiseFromRule(rule)
//   searchArgs.ts  searchInvokeArgs(params): the exact `search_anime` arguments (Discover, rows
//                  and smart collections all call it, so one rule gives one query)
//   watchOrder.ts  franchiseOrder(currentId, details): { state, entries, missing }
//   collage.ts     collageCovers(covers, max = 4)
//   savedNote.ts   savedAge(ageSeconds): { unit, count }
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { franchiseFromRule, ruleForFranchise, ruleFromSearch, searchFromRule } from '../src/lib/smartRule.ts';
import { searchInvokeArgs } from '../src/lib/searchArgs.ts';
import { franchiseOrder } from '../src/lib/watchOrder.ts';
import { collageCovers } from '../src/lib/collage.ts';
import { savedAge } from '../src/lib/savedNote.ts';

// ---- 1. Studio rule and the Discover studio filter give the same query ------------------------

const discover = new URLSearchParams('studio=MAPPA&genre=Action&format=TV&page=3');
const rule = ruleFromSearch(discover);
assert.ok(rule, 'a rule is built');
assert.ok(new URLSearchParams(rule).get('studio') === 'MAPPA', 'studio is kept in the rule');
assert.equal(new URLSearchParams(rule).get('page'), null, 'the page is not part of a rule');
assert.equal(searchFromRule(rule).studio, 'MAPPA');

// A rule that is only a studio is a real filter (not "the whole catalogue").
assert.equal(new URLSearchParams(ruleFromSearch(new URLSearchParams('studio=Bones'))).get('studio'), 'Bones');
// Studio names with spaces and punctuation survive the round trip; absurd ones are dropped.
for (const name of ['Studio Pierrot', 'A-1 Pictures', "J.C.Staff", 'Kyoto Animation']) {
  assert.equal(searchFromRule(ruleFromSearch(new URLSearchParams({ studio: name }))).studio, name);
}
assert.equal(searchFromRule(`studio=${'x'.repeat(81)}`).studio, undefined, 'over 80 characters');
assert.equal(searchFromRule('studio=%20%20').studio, undefined, 'blank');

// Discover's filters and the rule's search produce the very same `search_anime` arguments.
const fromDiscover = searchInvokeArgs({ studio: 'MAPPA', genres: ['Action'], format: 'TV', sort: 'POPULARITY_DESC', page: 1, perPage: 24 });
const fromRule = searchInvokeArgs(searchFromRule(rule));
assert.deepEqual(fromRule, fromDiscover);
assert.deepEqual(fromDiscover, {
  query: null,
  genres: ['Action'],
  format: 'TV',
  status: null,
  sort: 'POPULARITY_DESC',
  page: 1,
  perPage: 24,
  excludeGenres: null,
  minScore: null,
  year: null,
  season: null,
  studio: 'MAPPA',
});
assert.equal(searchInvokeArgs({}).studio, null, 'no studio sends null, as the other filters do');

// ---- 2. Franchise-order collections ----------------------------------------------------------

assert.equal(ruleForFranchise(154587), 'franchise=154587');
assert.equal(franchiseFromRule('franchise=154587'), 154587);
assert.equal(franchiseFromRule('genre=Action'), null);
for (const bad of ['franchise=0', 'franchise=-4', 'franchise=abc', 'franchise=1.5', 'franchise=99999999999999', 'franchise=']) {
  assert.equal(franchiseFromRule(bad), null, bad);
}
assert.throws(() => ruleForFranchise(0), 'only a real id makes a rule');
// A franchise rule is not a Discover filter: it never turns into a search.
assert.deepEqual(Object.values(searchFromRule('franchise=1')).filter((v) => v !== undefined), []);

const node = (id, extra = {}) => ({ id, title: { romaji: `S${id}` }, format: 'TV', seasonYear: 2000 + id, ...extra });
const edge = (relationType, n) => ({ relationType, node: n });
const full = (id, edges = []) => ({ ...node(id), relations: { edges } });

// Complete: 1 -> 2 -> 3 with all details fetched.
const complete = new Map([
  [1, full(1, [edge('SEQUEL', node(2))])],
  [2, full(2, [edge('PREQUEL', node(1)), edge('SEQUEL', node(3))])],
  [3, full(3, [edge('PREQUEL', node(2))])],
]);
let result = franchiseOrder(2, complete);
assert.equal(result.state, 'complete');
assert.deepEqual(result.entries.map((e) => e.node.id), [1, 2, 3]);
assert.deepEqual(result.missing, []);

// Partial: the chain is known but later details are not yet fetched (AniList down halfway).
const partial = new Map([[1, full(1, [edge('SEQUEL', node(2))])]]);
result = franchiseOrder(1, partial);
assert.equal(result.state, 'partial');
assert.deepEqual(result.entries.map((e) => e.node.id), [1, 2]);
assert.deepEqual(result.missing, [2]);

// Partial: relations are missing altogether (a fallback catalogue without relations): the
// anime itself is shown and the state says the rest is unknown.
const noRelations = new Map([[5, { ...node(5), relations: null }]]);
result = franchiseOrder(5, noRelations);
assert.equal(result.state, 'partial');
assert.deepEqual(result.entries.map((e) => e.node.id), [5]);
assert.deepEqual(result.missing, []);
const noKey = new Map([[5, node(5)]]);
assert.equal(franchiseOrder(5, noKey).state, 'partial', 'no relations key at all');

// A true "no franchise": relations were read and there are none -> complete, one entry.
const alone = new Map([[6, full(6, [])]]);
result = franchiseOrder(6, alone);
assert.deepEqual([result.state, result.entries.length], ['complete', 1]);

// Unavailable: nothing is known about the anime. No crash, no entries.
result = franchiseOrder(9, new Map());
assert.deepEqual([result.state, result.entries, result.missing], ['unavailable', [], [9]]);
// Hostile data (null nodes, self relations, loops) never loops forever or throws.
const loop = new Map([
  [1, full(1, [edge('SEQUEL', node(2)), edge('SEQUEL', null)])],
  [2, full(2, [edge('SEQUEL', node(1)), edge('PREQUEL', node(1))])],
]);
assert.ok(franchiseOrder(1, loop).entries.length <= 2);

// ---- 3. Cover collage -----------------------------------------------------------------------

const url = (n) => `https://img.example/${n}.jpg`;
assert.deepEqual(collageCovers([url(1), url(2), url(3), url(4), url(5)]), [url(1), url(2), url(3), url(4)], 'up to four');
assert.deepEqual(collageCovers([url(1), null, undefined, '', '   ', url(2)]), [url(1), url(2)], 'missing ones are skipped');
assert.deepEqual(collageCovers([url(1), url(1), url(2), url(1)]), [url(1), url(2)], 'a cover is used once');
assert.deepEqual(collageCovers([null, undefined]), []);
assert.deepEqual(collageCovers(null), []);
assert.deepEqual(collageCovers(undefined), []);
assert.deepEqual(collageCovers([url(1), url(2), url(3)], 2), [url(1), url(2)], 'a smaller maximum');
// Stable: the same input gives the same answer and the input is not changed.
const input = [url(3), null, url(1), url(2)];
const copy = [...input];
assert.deepEqual(collageCovers(input), collageCovers(input));
assert.deepEqual(input, copy);
assert.deepEqual(collageCovers(input), [url(3), url(1), url(2)], 'first come, first used');

// ---- 4. "Saved page" note: how old a saved copy is -------------------------------------------

assert.deepEqual(savedAge(0), { unit: 'now', count: 0 });
assert.deepEqual(savedAge(59), { unit: 'now', count: 0 });
assert.deepEqual(savedAge(60), { unit: 'minutes', count: 1 });
assert.deepEqual(savedAge(59 * 60 + 59), { unit: 'minutes', count: 59 });
assert.deepEqual(savedAge(3600), { unit: 'hours', count: 1 });
assert.deepEqual(savedAge(23 * 3600 + 3599), { unit: 'hours', count: 23 });
assert.deepEqual(savedAge(86400), { unit: 'days', count: 1 });
assert.deepEqual(savedAge(40 * 86400), { unit: 'days', count: 40 });
assert.deepEqual(savedAge(-5), { unit: 'now', count: 0 }, 'a clock that went backwards');
assert.deepEqual(savedAge(Number.NaN), { unit: 'now', count: 0 });

// ---- 5. The cover limit setting and the new texts --------------------------------------------

const hook = readFileSync(new URL('../src/hooks/useSettings.ts', import.meta.url), 'utf8');
assert.match(hook, /image_cache_mb:\s*200\b/, 'default 200 MB in useSettings.ts');
const types = readFileSync(new URL('../src/types/index.ts', import.meta.url), 'utf8');
assert.match(types, /image_cache_mb:\s*number/, 'typed in types/index.ts');

const KEYS = [
  'discover.studio', // the Discover filter's label
  'discover.studioPlaceholder',
  'smartCollection.studio', // how a studio rule is shown in a collection's filter summary
  'smartCollection.franchise', // a franchise rule, shown as its watch order
  'franchise.partial', // some entries of the order are not known yet
  'franchise.unavailable', // relations cannot be read while the catalogues are down
  'offline.savedPage', // "Saved page from {source}, {age}" (uses {source} and {age})
  'offline.savedPageNoSource',
  'offline.ageNow',
  'offline.ageMinutes',
  'offline.ageHours',
  'offline.ageDays',
  'offline.changesWaiting', // "{count} list changes will be sent when you are online"
  'settings.imageCacheLimit', // Settings > System row
  'settings.imageCacheLimitHint',
];
const dir = new URL('../src/i18n/locales/', import.meta.url);
for (const name of readdirSync(dir).filter((f) => f.endsWith('.json'))) {
  const strings = JSON.parse(readFileSync(new URL(name, dir), 'utf8'));
  // A text, or the plural forms of one (the age and count texts are plural keys).
  const filled = (v) => (typeof v === 'string' ? v : Object.values(v ?? {}).join('')).trim() !== '';
  for (const key of KEYS) assert.ok(filled(strings[key]), `${name} lacks ${key}`);
}
console.log('collections tests passed');
