// Checks the Home row rules (src/lib/homeRows.ts) and smart-collection rules (src/lib/smartRule.ts).
// Run with: node --experimental-strip-types scripts/home-test.mjs
import assert from 'node:assert/strict';
import {
  addHomeRow,
  isHomeRowId,
  livePlaylistRow,
  moveHomeRow,
  orderHomeRows,
  removeHomeRow,
  rowKind,
  toggleHiddenRow,
  visibleHomeRows,
} from '../src/lib/homeRows.ts';
import { discoverLink, ruleFromSearch, searchFromRule } from '../src/lib/smartRule.ts';

const FIVE = ['continue', 'because', 'trending', 'popular', 'top_rated'];

// Settings from 1.4 load unchanged: the five rows in the saved order, nothing hidden.
assert.deepEqual(orderHomeRows(undefined), FIVE);
assert.deepEqual(orderHomeRows(['top_rated', 'continue']), ['top_rated', 'continue', 'because', 'trending', 'popular']);
assert.deepEqual(visibleHomeRows(orderHomeRows(['popular']), undefined), ['popular', 'continue', 'because', 'trending', 'top_rated']);

// Added rows keep their place; unknown, repeated and malformed ids are dropped.
assert.deepEqual(
  orderHomeRows(['airing', 'status:PLANNING', 'playlist:pl-1a-0-ff', 'status:DROPPED', 'airing', 'playlist:../x', 42, 'continue']),
  ['airing', 'status:PLANNING', 'playlist:pl-1a-0-ff', 'continue', 'because', 'trending', 'popular', 'top_rated']
);
for (const bad of ['', 'status:', 'status:COMPLETED', 'playlist:', 'playlist:a b', `playlist:${'x'.repeat(65)}`, null, {}]) {
  assert.equal(isHomeRowId(bad), false, String(bad));
}

// Hide and show any row; the hidden list ignores junk.
const hidden = toggleHiddenRow(['nope', 7], 'trending');
assert.deepEqual(hidden, ['trending']);
assert.deepEqual(visibleHomeRows(FIVE, hidden), ['continue', 'because', 'popular', 'top_rated']);
assert.deepEqual(toggleHiddenRow(hidden, 'trending'), []);

// Add once, remove added rows only.
let order = addHomeRow(FIVE, 'airing');
assert.deepEqual(addHomeRow(order, 'airing'), order);
assert.deepEqual(addHomeRow(order, 'status:DROPPED'), order);
order = addHomeRow(order, 'playlist:pl-9');
assert.deepEqual(removeHomeRow(order, 'continue'), order);
assert.deepEqual(removeHomeRow(order, 'airing'), [...FIVE, 'playlist:pl-9']);
assert.deepEqual(moveHomeRow(order, 'playlist:pl-9', -1).slice(-2), ['playlist:pl-9', 'airing']);
assert.deepEqual(moveHomeRow(order, 'playlist:pl-9', 1), order);

// Row kinds, and playlist rows whose playlist is gone.
assert.deepEqual(rowKind('status:PAUSED'), { kind: 'status', status: 'PAUSED' });
assert.deepEqual(rowKind('playlist:pl-9'), { kind: 'playlist', playlistId: 'pl-9' });
assert.equal(livePlaylistRow('playlist:pl-9', ['pl-1']), false);
assert.equal(livePlaylistRow('playlist:pl-9', undefined), true);
assert.equal(livePlaylistRow('airing', []), true);

// Smart rules: only filters are kept, in a fixed order; a sort alone is no rule.
const rule = ruleFromSearch(new URLSearchParams('page=3&sort=SCORE_DESC&genre=Mecha&year=2020&genre=Drama&unwatched=1'));
assert.equal(rule, 'genre=Mecha&genre=Drama&year=2020&sort=SCORE_DESC');
assert.equal(ruleFromSearch(new URLSearchParams('sort=SCORE_DESC&page=2')), null);
assert.equal(discoverLink(rule), '/discover?genre=Mecha&genre=Drama&year=2020&sort=SCORE_DESC');
assert.deepEqual(searchFromRule(rule), {
  query: undefined,
  genres: ['Mecha', 'Drama'],
  excludeGenres: undefined,
  format: undefined,
  status: undefined,
  season: undefined,
  year: 2020,
  minScore: undefined,
  sort: 'SCORE_DESC',
});

// A hostile rule (from a file or a hand-edited database) never passes bad values on.
const hostile = searchFromRule('format=tv;drop&year=99999&score=0&sort=a%20b&genre=Action&xgenre=Action&season=FALL&q=' + 'x'.repeat(200));
assert.equal(hostile.format, undefined);
assert.equal(hostile.year, undefined);
assert.equal(hostile.minScore, undefined);
assert.equal(hostile.sort, undefined);
assert.equal(hostile.query, undefined);
assert.deepEqual(hostile.genres, ['Action']);
assert.equal(hostile.excludeGenres, undefined, 'a genre is never both wanted and left out');
assert.equal(hostile.season, 'FALL');
assert.equal(searchFromRule('genre=' + Array.from({ length: 30 }, (_, i) => `G${i}`).join('&genre=')).genres.length, 12);

console.log('home-test: ok');
