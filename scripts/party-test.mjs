#!/usr/bin/env node
// Tests for the watch party's page maths: host position, drift corrections, length mismatch,
// the offset, the code and error keys.
//   node --experimental-strip-types scripts/party-test.mjs
import assert from 'node:assert/strict';
import {
  ALIGN_LEAD_MS, DEAD_BAND_MS, MAX_OFFSET_MS, QUIET_AFTER_ALIGN_MS, durationDifferenceMs, hostPositionMs, isBufferedAhead,
  needsAlign, normalizeCode, partyErrorKey, stepOffset,
} from '../src/lib/watchParty.ts';

// Host position: still while paused, moving with age and speed while playing.
const sample = { paused: false, positionMs: 60_000, rate: 1, ageMs: 100, receivedAt: 1000 };
assert.equal(hostPositionMs(sample, 1000), 60_100);
assert.equal(hostPositionMs(sample, 1400), 60_500);
assert.equal(hostPositionMs({ ...sample, rate: 2 }, 1400), 61_000);
assert.equal(hostPositionMs({ ...sample, paused: true }, 9000), 60_000);
// A start scheduled ahead: before it, the host is "behind" its start position; never below 0.
assert.equal(hostPositionMs({ ...sample, ageMs: -ALIGN_LEAD_MS }, 1000), 59_000);
assert.equal(hostPositionMs({ ...sample, positionMs: 0, ageMs: -1000 }, 1000), 0);

// Drift: the dead band, a paused host, a paused guest, the quiet window after a correction.
const check = { expectedMs: 10_000, actualMs: 10_000, actualPaused: false, hostPaused: false, sinceAlignMs: 5000 };
assert.equal(needsAlign(check), false);
assert.equal(needsAlign({ ...check, actualMs: 10_000 + DEAD_BAND_MS }), false);
assert.equal(needsAlign({ ...check, actualMs: 10_000 + DEAD_BAND_MS + 1 }), true);
assert.equal(needsAlign({ ...check, actualMs: 9_000 }), true);
assert.equal(needsAlign({ ...check, actualMs: 9_000, sinceAlignMs: QUIET_AFTER_ALIGN_MS - 1 }), false);
assert.equal(needsAlign({ ...check, actualPaused: true, sinceAlignMs: 0 }), true, 'a paused guest of a playing host');
assert.equal(needsAlign({ ...check, hostPaused: true }), true, 'a playing guest of a paused host');
assert.equal(needsAlign({ ...check, hostPaused: true, actualPaused: true }), false);
assert.equal(needsAlign({ ...check, hostPaused: true, actualPaused: true, actualMs: 12_000, sinceAlignMs: 0 }), true);

// Length mismatch beyond 2 s, signed from the guest's side.
assert.equal(durationDifferenceMs(1_420_000, 1420), null);
assert.equal(durationDifferenceMs(1_420_000, 1421.9), null);
assert.equal(durationDifferenceMs(1_420_000, 1426), 6000);
assert.equal(durationDifferenceMs(1_420_000, 1410), -10_000);
assert.equal(durationDifferenceMs(null, 1410), null);
assert.equal(durationDifferenceMs(1_420_000, Number.NaN), null);

// Offset steps stay within ±30 s.
assert.equal(stepOffset(0, 1), 500);
assert.equal(stepOffset(0, -1), -500);
assert.equal(stepOffset(MAX_OFFSET_MS, 1), MAX_OFFSET_MS);
assert.equal(stepOffset(-MAX_OFFSET_MS, -1), -MAX_OFFSET_MS);

// Ready: the next 2 s buffered, or up to the end; a range that starts after the position does not count.
assert.equal(isBufferedAhead([[0, 12.5]], 10, 60), true);
assert.equal(isBufferedAhead([[0, 11]], 10, 60), false);
assert.equal(isBufferedAhead([[10.5, 30]], 10, 60), false);
assert.equal(isBufferedAhead([[0, 5], [9.98, 13]], 10, 60), true);
assert.equal(isBufferedAhead([[50, 59]], 58.5, 59), true, 'near the end');
assert.equal(isBufferedAhead([], 10, Number.NaN), false);

// Codes and error keys.
assert.equal(normalizeCode('123 456'), '123456');
assert.equal(normalizeCode('123-456'), '123456');
for (const bad of ['12345', '1234567', 'abcdef', '', '١٢٣٤٥٦']) assert.equal(normalizeCode(bad), null, bad);
assert.equal(partyErrorKey('party:wrong_code'), 'party.errors.wrong_code');
assert.equal(partyErrorKey('Network error: x'), null);
assert.equal(partyErrorKey('party:../x'), null);

console.log('party: ok');
