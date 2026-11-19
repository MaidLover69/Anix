#!/usr/bin/env node
// #10 §2: preferred resolution for streaming and downloads (pure rules).
//   node --experimental-strip-types scripts/resolution-test.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {
  DOWNLOAD_QUALITY_VALUES, STREAMING_RESOLUTION_VALUES, pickHeight, startLevelIndex, wantedHeight,
} from '../src/lib/resolution.ts';

assert.deepEqual(STREAMING_RESOLUTION_VALUES, ['auto', '1080', '720', '480', '360']);
assert.deepEqual(DOWNLOAD_QUALITY_VALUES, ['best', '1080p', '720p', '480p', '360p'], 'download_quality keeps its 1.4 values');

// A setting reads as a height; auto, best and anything unknown mean no constraint.
for (const [value, height] of [['720', 720], ['720p', 720], [720, 720], ['1080p', 1080], ['360', 360], [' 480P ', 480]]) {
  assert.equal(wantedHeight(value), height, String(value));
}
for (const value of ['auto', 'best', '', undefined, null, 'abc', '240', 0, NaN]) assert.equal(wantedHeight(value), null, String(value));

// Exact, else the closest lower, else the closest higher.
assert.equal(pickHeight([1080, 720, 360], 720), 720);
assert.equal(pickHeight([1080, 360], 720), 360, 'closest lower');
assert.equal(pickHeight([1080, 720, 480], 360), 480, 'nothing lower: closest higher');
assert.equal(pickHeight([1080, 720], 360), 720);
assert.equal(pickHeight([480], 1080), 480);
assert.equal(pickHeight([360, 1080], 480), 360);
assert.equal(pickHeight([1080, 720], null), null, 'no constraint');
assert.equal(pickHeight([], 720), null);

// hls.js start level: index into the level list, -1 (Auto) when nothing is wanted or known.
const levels = [{ height: 360, bitrate: 500 }, { height: 720, bitrate: 2000 }, { height: 720, bitrate: 3000 }, { height: 1080, bitrate: 5000 }];
assert.equal(startLevelIndex(levels, 720), 2, 'the best bitrate at the matching height');
assert.equal(startLevelIndex(levels, 1080), 3);
assert.equal(startLevelIndex(levels, 480), 0, 'closest lower');
assert.equal(startLevelIndex(levels, null), -1);
assert.equal(startLevelIndex([], 720), -1);
assert.equal(startLevelIndex([{ height: 0, bitrate: 100 }, { height: 0, bitrate: 200 }], 720), -1, 'levels without a height give no basis');
assert.equal(startLevelIndex([{ height: 0, bitrate: 100 }, { height: 480, bitrate: 200 }], 720), 1);

// Defaults in the settings (1.4 settings files lack the new key and read as auto).
const defaults = fs.readFileSync(new URL('../src/hooks/useSettings.ts', import.meta.url), 'utf8');
assert.match(defaults, /streaming_resolution:\s*'auto'/);
assert.match(defaults, /download_quality:\s*'best'/);
assert.match(defaults, /default_playback_speed:\s*1\b/);
console.log('resolution tests passed');
