#!/usr/bin/env node
// Tests for the embedded mpv stand-in (MpvMedia) and the subtitle choice at load.
//   node --experimental-strip-types scripts/player-test.mjs
import assert from 'node:assert/strict';
import { MpvMedia } from '../src/components/player/mpvMedia.ts';
import { addedTrackId, fileSubtitleTracks, providerIndex, subtitleAtLoad, subtitleCorrection } from '../src/lib/mpvSubtitles.ts';

/** A stand-in with a record of what it sent to mpv and which events it fired. */
function player() {
  const sent = [];
  const fired = [];
  const media = new MpvMedia((action) => sent.push(action));
  for (const name of ['play', 'playing', 'pause', 'waiting', 'canplay', 'ended', 'seeking', 'seeked', 'loadedmetadata']) {
    media.addEventListener(name, () => fired.push(name));
  }
  return { media, sent, fired, clear: () => { sent.length = 0; fired.length = 0; } };
}
const loaded = (paused) => ({ kind: 'loaded', tracks: [], paused });

// A1: mpv is already playing when the file opens; it never reports `pause` again.
{
  const { media, fired } = player();
  assert.equal(media.paused, true, 'nothing open yet');
  media.handle(loaded(false));
  assert.equal(media.paused, false, 'the load reports that mpv plays');
  assert.ok(fired.includes('play') && fired.includes('playing'));
}
{
  const { media, fired } = player();
  media.handle(loaded(true));
  assert.equal(media.paused, true);
  assert.ok(!fired.includes('play'), 'no play event while mpv is paused');
}
{
  const { media } = player();
  media.handle(loaded(null));
  assert.equal(media.paused, true, 'an unknown state leaves the stand-in as it was');
}
// A1: play() and pause() take effect at once, so a set_pause that changes nothing in mpv
// (no property event follows) cannot leave the controls out of step.
{
  const { media, sent, fired, clear } = player();
  media.handle(loaded(true));
  clear();
  media.play();
  assert.equal(media.paused, false);
  assert.deepEqual(sent, [{ action: 'set_pause', paused: false }]);
  assert.deepEqual(fired, ['play', 'playing']);
  clear();
  media.pause();
  assert.equal(media.paused, true);
  assert.deepEqual(sent, [{ action: 'set_pause', paused: true }]);
  assert.deepEqual(fired, ['pause']);
  clear();
  media.handle({ kind: 'property', name: 'pause', value: true });
  assert.deepEqual(fired, [], "mpv's later report of the same state fires nothing");
  media.handle({ kind: 'property', name: 'pause', value: false });
  assert.equal(media.paused, false, 'a pause change from mpv (MPRIS, keys) still applies');
}

// A3: buffering is not a pause. The stand-in stays "playing" and reports waiting, then playing.
{
  const { media, fired, clear } = player();
  media.handle(loaded(false));
  clear();
  media.handle({ kind: 'property', name: 'paused-for-cache', value: true });
  assert.equal(media.paused, false, 'buffering keeps the play state');
  assert.deepEqual(fired, ['waiting']);
  media.pause();
  clear();
  media.play();
  assert.deepEqual(fired, ['play'], 'pressing play while buffering does not hide the spinner');
  clear();
  media.handle({ kind: 'property', name: 'paused-for-cache', value: false });
  assert.deepEqual(fired, ['playing']);
}

// A4: after the end mpv has unloaded the file, so play and seek open it again.
{
  const { media, sent, clear } = player();
  media.src = 'http://127.0.0.1:1/t/proxy?url=a';
  media.handle(loaded(false));
  media.handle({ kind: 'property', name: 'duration', value: 1400 });
  media.handle({ kind: 'end', reason: 'eof' });
  assert.equal(media.ended, true);
  assert.equal(media.paused, true);
  clear();
  media.play();
  assert.deepEqual(sent, [
    { action: 'set_pause', paused: false },
    { action: 'load', url: 'http://127.0.0.1:1/t/proxy?url=a', start: null },
  ]);
  assert.equal(media.paused, false);
  assert.equal(media.ended, false);
  assert.equal(media.currentTime, 0);
  assert.equal(media.replaying, true, 'the hook skips its resume check for this load');
  media.handle(loaded(false));
  assert.equal(media.replaying, false);
}
{
  const { media, sent, clear } = player();
  media.src = 'http://127.0.0.1:1/t/proxy?url=a';
  media.handle(loaded(false));
  media.handle({ kind: 'property', name: 'duration', value: 1400 });
  media.handle({ kind: 'end', reason: 'eof' });
  clear();
  media.currentTime = 600;
  assert.deepEqual(sent, [
    { action: 'set_pause', paused: true },
    { action: 'load', url: 'http://127.0.0.1:1/t/proxy?url=a', start: 600 },
  ], 'a seek after the end reopens the file there, paused like an element');
  assert.equal(media.currentTime, 600);
  clear();
  media.currentTime = 700;
  assert.deepEqual(sent, [{ action: 'seek', seconds: 700, absolute: true }], 'once open again, seeks are plain seeks');
}
{
  const { media, sent, clear } = player();
  media.handle(loaded(false));
  clear();
  media.currentTime = 30;
  assert.deepEqual(sent, [{ action: 'seek', seconds: 30, absolute: true }], 'before the end, a seek is a seek');
}

// A2: only the shown subtitle file is added when a video opens.
{
  const OFF = 'off';
  assert.deepEqual(subtitleAtLoad(undefined, OFF, 9, 2, false), { kind: 'add', index: 2 }, 'first load: the default file');
  assert.deepEqual(subtitleAtLoad(undefined, OFF, 9, 2, true), { kind: 'off' }, 'a dub starts with none');
  assert.deepEqual(subtitleAtLoad(undefined, OFF, 0, -1, false), { kind: 'follow' }, 'no files: what mpv shows');
  assert.deepEqual(subtitleAtLoad('ext:5', OFF, 9, 2, false), { kind: 'add', index: 5 }, 'a reload keeps the picked file');
  assert.deepEqual(subtitleAtLoad('ext:12', OFF, 9, 2, false), { kind: 'follow' }, 'an index past the list is not added');
  assert.deepEqual(subtitleAtLoad(OFF, OFF, 9, 2, false), { kind: 'off' });
  assert.deepEqual(subtitleAtLoad('mpv:3', OFF, 9, 2, false), { kind: 'follow' });
  assert.equal(providerIndex('ext:4'), 4);
  assert.equal(providerIndex('mpv:4'), null);
  assert.equal(providerIndex('ext:x'), null);

  const tracks = [
    { id: 1, type: 'video', lang: null, title: null, codec: 'h264', selected: true },
    { id: 1, type: 'sub', lang: 'jpn', title: null, codec: 'ass', selected: false, external_filename: null },
    { id: 2, type: 'sub', lang: null, title: 'English', codec: 'ass', selected: true, external_filename: 'http://p/s1' },
  ];
  assert.equal(addedTrackId(tracks, 'http://p/s1'), 2);
  assert.equal(addedTrackId(tracks, 'http://p/s2'), null, 'not added yet');
  assert.deepEqual(fileSubtitleTracks(tracks).map((t) => t.id), [1], "added files are listed from the extension's list");
}

// Review follow-ups: a seek that ends during a stall keeps the spinner; a load or an end clears the stall.
{
  const { media, fired, clear } = player();
  media.handle(loaded(false));
  media.handle({ kind: 'property', name: 'paused-for-cache', value: true });
  clear();
  media.handle({ kind: 'seeked' });
  assert.deepEqual(fired, ['seeked', 'waiting'], 'still filling the cache after the seek');
  media.handle(loaded(false));
  clear();
  media.handle({ kind: 'seeked' });
  assert.deepEqual(fired, ['seeked', 'playing'], 'a new file starts without the old stall');
}
// A replay after the end shows buffering until mpv opened the file again.
{
  const { media, fired, clear } = player();
  media.src = 'http://p/v.m3u8';
  media.handle(loaded(false));
  media.handle({ kind: 'end', reason: 'eof', error: null });
  clear();
  media.play();
  assert.equal(fired[fired.length - 1], 'waiting');
  assert.equal(media.readyState, 0);
  assert.equal(media.replaying, true);
  media.handle({ kind: 'end', reason: 'error', error: 'gone' });
  assert.equal(media.replaying, false, 'a failed reopen is over');
}
// A file that finished downloading after the user chose something else is switched back.
{
  const urlOf = (i) => `http://p/s${i}`;
  const t = (selected) => [
    { id: 1, type: 'sub', selected: selected === 1, external_filename: null },
    { id: 2, type: 'sub', selected: selected === 2, external_filename: 'http://p/s0' },
    { id: 3, type: 'sub', selected: selected === 3, external_filename: 'http://p/s1' },
  ];
  assert.equal(subtitleCorrection(t(2), 'off', urlOf), null, 'Off picked while s0 was downloading');
  assert.equal(subtitleCorrection(t(null), 'off', urlOf), undefined, 'already off');
  assert.equal(subtitleCorrection(t(2), 'ext:1', urlOf), 3, 'the later pick wins');
  assert.equal(subtitleCorrection(t(3), 'ext:1', urlOf), undefined, 'already shown');
  assert.equal(subtitleCorrection(t(2), 'ext:5', urlOf), undefined, 'still downloading: its own select shows it');
  assert.equal(subtitleCorrection(t(2), 'mpv:1', urlOf), 1, "the video file's own track");
}

console.log('player tests passed');
