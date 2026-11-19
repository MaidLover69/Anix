#!/usr/bin/env node
// #10 §3: fixed and custom speeds, the default speed, and the Meme mode High speed labels.
// playerLogic.ts imports without extensions, so node cannot load it: the speed rules live in src/lib/speed.ts (playerLogic re-exports MIN_SPEED, MAX_SPEED, SPEED_STEP, stepSpeed from there).
//   node --experimental-strip-types scripts/speed-test.mjs
import assert from 'node:assert/strict';
import {
  CUSTOM_SPEED_MAX, CUSTOM_SPEED_MIN, CUSTOM_SPEED_STEP, FIXED_SPEEDS, MAX_SPEED,
  episodeStartRate, isFixedSpeed, parseCustomSpeed, speedSpot, stepSpeed,
} from '../src/lib/speed.ts';
import { LINES } from '../src/lib/memeCatalogue.ts';

// Fixed steps and the [ ] keys do not change.
assert.deepEqual(FIXED_SPEEDS, [0.5, 0.75, 1, 1.25, 1.5, 2, 3]);
assert.equal(MAX_SPEED, 3);
assert.equal(stepSpeed(2, 1), 3);
assert.equal(stepSpeed(3, 1), 3, '[ ] stop at 3');
assert.equal(stepSpeed(1, -1), 0.75);
assert.equal(isFixedSpeed(1.5), true);
assert.equal(isFixedSpeed(2.35), false);
assert.equal(isFixedSpeed(4), false);

// Custom speed: 0.5 to 4 in 0.05 steps; in-range input is rounded to the grid, out-of-range input is refused.
assert.equal(CUSTOM_SPEED_MIN, 0.5);
assert.equal(CUSTOM_SPEED_MAX, 4);
assert.equal(CUSTOM_SPEED_STEP, 0.05);
const accepted = [[2.35, 2.35], ['2.35', 2.35], [' 4 ', 4], [4, 4], [0.5, 0.5], [1.15, 1.15], [2.37, 2.35], [2.38, 2.4], [1, 1]];
for (const [input, expected] of accepted) assert.equal(parseCustomSpeed(input), expected, String(input));
const refused = [0.45, 0.49, 4.05, 5, 0, -1, NaN, Infinity, 'abc', '', '2.3.5', null, undefined, {}, true];
for (const input of refused) assert.equal(parseCustomSpeed(input), null, String(input));

// Every episode starts at the default speed; a speed changed in the last one is not an input.
assert.equal(episodeStartRate(1.5), 1.5);
assert.equal(episodeStartRate(2.35), 2.35);
assert.equal(episodeStartRate(undefined), 1);
assert.equal(episodeStartRate(9), 1, 'out of range: 1×');
assert.equal(episodeStartRate('abc'), 1);
assert.equal(episodeStartRate.length, 1, 'takes the default setting only, never the last episode’s speed');

// Meme mode High: the speed menu's captions (names, so the same in every language).
const spots = { 0.5: 'spd05', 0.75: 'spd075', 1: null, 1.25: 'spd125', 1.5: 'spd15', 2: 'spd2', 3: 'spd3', 2.35: 'spdCustom', 4: 'spdCustom' };
for (const [rate, spot] of Object.entries(spots)) assert.equal(speedSpot(Number(rate)), spot, rate);
const labels = {
  spd05: 'Square Accel', spd075: 'Tsukuyomi', spd125: 'Koro Koro', spd15: 'Yellow Flash',
  spd2: 'Pika Pika no Mi', spd3: 'Made in Heaven', spdCustom: 'Jikan Mahō',
};
for (const [spot, line] of Object.entries(labels)) {
  assert.deepEqual(LINES[spot], { line, keep: true }, spot);
}
console.log('speed tests passed');
