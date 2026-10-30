#!/usr/bin/env node
// Tests for when a player visit counts as watch history (10 s of real playback or 2 %).
//   node --experimental-strip-types scripts/history-test.mjs
import assert from 'node:assert/strict';
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

console.log('history tests passed');
