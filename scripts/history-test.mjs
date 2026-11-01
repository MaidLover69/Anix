#!/usr/bin/env node
// Tests for when a player visit counts as watch history (10 s of real playback or 2 %), and which
// episodes the download dialog queues.
//   node --experimental-strip-types scripts/history-test.mjs
import assert from 'node:assert/strict';
import { register } from 'node:module';
import { hasPlayedEnough, playedStep } from '../src/hooks/watchProgress.ts';

const EPISODE = 24 * 60;

// A one-second visit to a normal episode leaves no trace; ten seconds do.
assert.equal(hasPlayedEnough(0, EPISODE), false);
assert.equal(hasPlayedEnough(1, EPISODE), false);
assert.equal(hasPlayedEnough(9.9, EPISODE), false);
assert.equal(hasPlayedEnough(10, EPISODE), true);

// A short clip: 2 % comes first (3.6 s of a 3-minute clip).
assert.equal(hasPlayedEnough(3, 180), false);
assert.equal(hasPlayedEnough(3.6, 180), true);

// No known duration: nothing to save yet.
assert.equal(hasPlayedEnough(100, 0), false);
assert.equal(hasPlayedEnough(100, NaN), false);

// Only small forward steps while playing count; seeks, skips and steps back do not.
assert.equal(playedStep(10, 10.25), 0.25);
assert.equal(playedStep(10, 12), 2);
assert.equal(playedStep(10, 95), 0, 'skip intro');
assert.equal(playedStep(600, 10), 0, 'seek back');
assert.equal(playedStep(10, 10), 0);
assert.equal(playedStep(NaN, 1), 0);

// Accumulated like the hook does: a resume jump to 12:00, then 1 s of playback, is not enough.
{
  let played = 0;
  let last = 0;
  for (const time of [0, 720, 720.25, 720.5, 720.75, 721]) {
    played += playedStep(last, time);
    last = time;
  }
  assert.equal(played, 1);
  assert.equal(hasPlayedEnough(played, EPISODE), false);
}
// Twelve seconds at 4 readings a second is enough.
{
  let played = 0;
  let last = 0;
  for (let i = 1; i <= 48; i += 1) {
    played += playedStep(last, i / 4);
    last = i / 4;
  }
  assert.equal(hasPlayedEnough(played, EPISODE), true);
}

// Which episodes the download dialog queues (src/lib/downloadPicks.ts). Its `./languages` import
// pulls in React and the locale files, so it is stubbed; the picks never use it.
register(
  'data:text/javascript,' +
    encodeURIComponent(
      `export async function resolve(s, c, next) {
        return s === './languages'
          ? { url: 'data:text/javascript,export const sourceLabel=()=>"";export const versionLabel=()=>"";', shortCircuit: true }
          : next(s, c);
      }`
    )
);
const { nextUnwatched, pickEpisodes, storageState } = await import('../src/lib/downloadPicks.ts');
const pick = (input) =>
  pickEpisodes({ pick: 'range', total: 12, from: 1, to: 12, watched: [], saved: [], filler: [], skipFiller: false, ...input });

// A range is inclusive, and a reversed one counts the same episodes.
assert.deepEqual(pick({ from: 3, to: 5 }), [3, 4, 5]);
assert.deepEqual(pick({ from: 12, to: 3 }), [3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
assert.deepEqual(pick({ from: null }), [], 'half-typed range');
// Filler is left out only when asked; saved or queued episodes are always left out.
assert.deepEqual(pick({ from: 1, to: 6, filler: [2, 3], saved: [3, 5] }), [1, 2, 4, 6]);
assert.deepEqual(pick({ from: 1, to: 6, filler: [2, 3], saved: [3, 5], skipFiller: true }), [1, 4, 6]);
// "Unwatched" walks the known total, and has nothing to walk without one.
assert.deepEqual(pick({ pick: 'unwatched', total: 5, watched: [1, 2, 4], saved: [5] }), [3]);
assert.deepEqual(pick({ pick: 'unwatched', total: null }), []);
// The limit keeps one extra pick, so the caller can tell the batch is over it.
assert.deepEqual(pick({ from: 1, to: 12, limit: 3 }), [1, 2, 3, 4]);
assert.deepEqual(pick({ from: 1, to: 12, limit: 3, saved: [2] }), [1, 3, 4, 5]);
// A huge "to" with no known total stops at the limit instead of building a billion-entry list.
{
  const started = performance.now();
  const picked = pick({ total: null, from: 1, to: 999_999_999, limit: 100 });
  assert.equal(picked.length, 101);
  assert.equal(picked.at(-1), 101);
  assert.ok(performance.now() - started < 1000, 'huge range finishes fast');
}

// "From episode N" is the one after the last watched, if there is one.
assert.equal(nextUnwatched([], 12), null);
assert.equal(nextUnwatched([1, 2, 3.5], 12), 4);
assert.equal(nextUnwatched([12], 12), null, 'all watched');
assert.equal(nextUnwatched([40], null), 41);

// Free space: more than free is full, less than the headroom left is tight.
assert.equal(storageState(11, 10, 2), 'full');
assert.equal(storageState(10, 10, 2), 'tight');
assert.equal(storageState(9, 10, 2), 'tight');
assert.equal(storageState(8, 10, 2), 'ok');

// Schedule › Up next: soonest first, one per anime, only from the list, only still to air. The
// countdown text needs the locale files, so `t` is stubbed; upNext never calls it.
register(
  'data:text/javascript,' +
    encodeURIComponent(
      `export async function resolve(s, c, next) {
        return s === '../../i18n'
          ? { url: 'data:text/javascript,export const t=()=>"";', shortCircuit: true }
          : next(s, c);
      }`
    )
);
const { upNext } = await import('../src/components/schedule/timeline.ts');
{
  const e = (media_id, airing_at) => ({ media_id, airing_at, format: null });
  const all = [e(1, 300), e(2, 200), e(1, 150), e(3, 50), e(4, 400), e(5, 250)];
  const mine = new Set([1, 2, 3, 4]);
  assert.deepEqual(upNext(all, mine, 100, 4).map((x) => [x.media_id, x.airing_at]), [[1, 150], [2, 200], [4, 400]]);
  assert.deepEqual(upNext(all, mine, 100).map((x) => x.media_id), [1], 'one by default');
  assert.deepEqual(upNext(all, new Set(), 100, 4), [], 'nothing on the list');
}

// Notifications › Today / Earlier: split at local midnight, order kept.
const { splitByDay } = await import('../src/lib/time.ts');
{
  const now = new Date(2026, 9, 31, 10, 0).getTime();
  const midnight = new Date(2026, 9, 31).getTime();
  const items = [{ created_at: now }, { created_at: midnight }, { created_at: midnight - 1 }, { created_at: 0 }];
  const { today, earlier } = splitByDay(items, now);
  assert.deepEqual(today, items.slice(0, 2));
  assert.deepEqual(earlier, items.slice(2));
}

console.log('history tests passed');
