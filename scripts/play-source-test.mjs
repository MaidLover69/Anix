#!/usr/bin/env node
// Issue #9: a downloaded episode plays from its file unless the user picks another server for
// that episode. Also: the new Settings switch has text in every language.
//   node --experimental-strip-types scripts/play-source-test.mjs
//
// Seam: src/lib/playSource.ts (pure, no imports from hooks):
//   playsFromFile({ hasLocalFile, explicitProvider, rememberedProvider?, preferredProvider? }): boolean
// Detail.tsx handlePlay, EpisodeBrowser.tsx onSelect and Watch.tsx use them instead of their own rules.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { playsFromFile } from '../src/lib/playSource.ts';

const base = { hasLocalFile: true, explicitProvider: null };

// A file plays by default, whatever is remembered or preferred.
assert.equal(playsFromFile(base), true);
assert.equal(playsFromFile({ ...base, rememberedProvider: 'miruro' }), true, 'remembered server');
assert.equal(playsFromFile({ ...base, preferredProvider: 'anizone' }), true, 'preferred server in Settings');
assert.equal(playsFromFile({ ...base, rememberedProvider: 'miruro', preferredProvider: 'anizone' }), true);

// Auto and "Your files" picked explicitly still mean the file.
assert.equal(playsFromFile({ ...base, explicitProvider: 'auto' }), true);
assert.equal(playsFromFile({ ...base, explicitProvider: 'local' }), true);

// Picking another server for this episode streams it.
assert.equal(playsFromFile({ ...base, explicitProvider: 'miruro' }), false);
assert.equal(playsFromFile({ ...base, explicitProvider: 'miruro', rememberedProvider: 'local' }), false);

// No file, nothing to play from.
assert.equal(playsFromFile({ hasLocalFile: false, explicitProvider: null }), false);
assert.equal(playsFromFile({ hasLocalFile: false, explicitProvider: 'local' }), false);

// The Settings switch "Download all subtitle languages" is translated everywhere.
const dir = new URL('../src/i18n/locales/', import.meta.url);
for (const name of readdirSync(dir).filter((f) => f.endsWith('.json'))) {
  const strings = JSON.parse(readFileSync(new URL(name, dir), 'utf8'));
  for (const key of ['settings.downloadAllSubtitles', 'settings.downloadAllSubtitlesHint']) {
    assert.ok(typeof strings[key] === 'string' && strings[key].trim(), `${name} lacks ${key}`);
  }
}
console.log('play-source tests passed');
