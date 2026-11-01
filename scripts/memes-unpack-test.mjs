// Tests for scripts/memes-unpack.mjs (the meme pack unpacker).
//   node --test scripts/memes-unpack-test.mjs
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import zlib from 'node:zlib';
import { readZip, unpack } from './memes-unpack.mjs';

const WEBP = Buffer.concat([Buffer.from('RIFF\x10\x00\x00\x00WEBPVP8 ', 'latin1'), Buffer.alloc(32, 7)]);
const MANIFEST = Buffer.from('{"memes":[]}');

/** A zip with one deflated entry per item; `mode` is the unix file mode, `size` overrides the declared size. */
function makeZip(items) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const { name, data, mode = 0o100644, size = data.length } of items) {
    const packed = zlib.deflateRawSync(data);
    const nameBuf = Buffer.from(name);
    const head = Buffer.alloc(30);
    head.writeUInt32LE(0x04034b50, 0);
    head.writeUInt16LE(20, 4);
    head.writeUInt16LE(8, 8);
    head.writeUInt32LE(zlib.crc32(data), 14);
    head.writeUInt32LE(packed.length, 18);
    head.writeUInt32LE(size, 22);
    head.writeUInt16LE(nameBuf.length, 26);
    const cd = Buffer.alloc(46);
    cd.writeUInt32LE(0x02014b50, 0);
    cd.writeUInt16LE((3 << 8) | 20, 4);
    cd.writeUInt16LE(20, 6);
    cd.writeUInt16LE(8, 10);
    cd.writeUInt32LE(zlib.crc32(data), 16);
    cd.writeUInt32LE(packed.length, 20);
    cd.writeUInt32LE(size, 24);
    cd.writeUInt16LE(nameBuf.length, 28);
    cd.writeUInt32LE((mode << 16) >>> 0, 38);
    cd.writeUInt32LE(offset, 42);
    locals.push(head, nameBuf, packed);
    central.push(cd, nameBuf);
    offset += 30 + nameBuf.length + packed.length;
  }
  const cdBuf = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(items.length, 8);
  end.writeUInt16LE(items.length, 10);
  end.writeUInt32LE(cdBuf.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cdBuf, end]);
}

const good = () => [
  { name: 'memes/manifest.json', data: MANIFEST },
  { name: 'memes/goku-kamehameha.webp', data: WEBP },
  { name: 'memes/naruto-run.still.webp', data: WEBP },
];

function setup(zip, hash = crypto.createHash('sha256').update(zip).digest('hex')) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'anix-memes-'));
  const paths = {
    zipPath: path.join(dir, 'memes.zip'),
    shaPath: path.join(dir, 'memes.zip.sha256'),
    outDir: path.join(dir, 'public/memes'),
    stampPath: path.join(dir, 'cache/stamp'),
  };
  fs.writeFileSync(paths.zipPath, zip);
  fs.writeFileSync(paths.shaPath, `${hash}  memes.zip\n`);
  return { dir, paths };
}

test('a good pack is written, and a second run is up to date', (t) => {
  const { dir, paths } = setup(makeZip(good()));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  assert.equal(unpack(paths), 'unpacked');
  assert.deepEqual(fs.readdirSync(paths.outDir).sort(), ['goku-kamehameha.webp', 'manifest.json', 'naruto-run.still.webp']);
  assert.deepEqual(fs.readFileSync(path.join(paths.outDir, 'goku-kamehameha.webp')), WEBP);
  assert.equal(unpack(paths), 'up-to-date');
  assert.deepEqual(fs.readdirSync(dir).sort(), ['cache', 'memes.zip', 'memes.zip.sha256', 'public'], 'no temp folder left');
});

test('a lost image is put back, and a stray temp folder is removed', (t) => {
  const { dir, paths } = setup(makeZip(good()));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  assert.equal(unpack(paths), 'unpacked');
  fs.rmSync(path.join(paths.outDir, 'goku-kamehameha.webp'));
  fs.mkdirSync(`${paths.outDir}.tmp-99999`);
  assert.equal(unpack(paths), 'unpacked');
  assert.deepEqual(fs.readFileSync(path.join(paths.outDir, 'goku-kamehameha.webp')), WEBP);
  assert.deepEqual(fs.readdirSync(path.join(dir, 'public')), ['memes']);
});

test('a wrong hash throws and leaves the output untouched', (t) => {
  const { dir, paths } = setup(makeZip(good()), 'a'.repeat(64));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.mkdirSync(paths.outDir, { recursive: true });
  fs.writeFileSync(path.join(paths.outDir, 'old.webp'), 'old');
  assert.throws(() => unpack(paths), /SHA-256/);
  assert.deepEqual(fs.readdirSync(paths.outDir), ['old.webp']);
  assert.equal(fs.existsSync(paths.stampPath), false);
});

test('a bad entry stops the unpack before anything is written', (t) => {
  const { dir, paths } = setup(makeZip([...good(), { name: 'memes/run.sh', data: Buffer.from('#!/bin/sh') }]));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  assert.throws(() => unpack(paths), /unexpected entry/);
  assert.equal(fs.existsSync(paths.outDir), false);
  assert.equal(fs.existsSync(path.join(dir, 'public')), false, 'no temp folder either');
});

test('entries outside memes/ or escaping it are refused', () => {
  for (const name of ['memes/../x.webp', '../x.webp', '/memes/x.webp', 'memes\\x.webp', 'other/a.webp', 'memes/sub/a.webp', 'memes/A.webp']) {
    assert.throws(() => readZip(makeZip([...good(), { name, data: WEBP }])), /unexpected entry/, name);
  }
});

test('foreign files, symlinks, duplicates, oversize and fake images are refused', () => {
  assert.throws(() => readZip(makeZip([...good(), { name: 'memes/run.sh', data: WEBP }])), /unexpected entry/);
  assert.throws(() => readZip(makeZip([...good(), { name: 'memes/link.webp', data: WEBP, mode: 0o120777 }])), /not a regular file/);
  assert.throws(() => readZip(makeZip([...good(), { name: 'memes/goku-kamehameha.webp', data: WEBP }])), /duplicate/);
  assert.throws(() => readZip(makeZip([...good(), { name: 'memes/big.webp', data: WEBP, size: (1 << 20) + 1 }])), /over/);
  assert.throws(() => readZip(makeZip([...good(), { name: 'memes/fake.webp', data: Buffer.from('<svg onload=x>') }])), /not WebP/);
  assert.throws(() => readZip(makeZip([{ name: 'memes/a.webp', data: WEBP }])), /manifest\.json is missing/);
  assert.throws(() => readZip(makeZip([{ name: 'memes/manifest.json', data: Buffer.from('{nope') }])), /not JSON/);
  assert.throws(() => readZip(Buffer.from('not a zip at all, just some bytes here')), /not a zip/);
});

test('a declared size that does not match the data is refused', () => {
  assert.throws(() => readZip(makeZip([...good(), { name: 'memes/short.webp', data: WEBP, size: WEBP.length - 1 }])), /size differs|buffer|Cannot create/i);
});

test('the committed pack matches its hash and holds only images and the manifest', () => {
  const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const zip = fs.readFileSync(path.join(app, 'assets/memes.zip'));
  const want = fs.readFileSync(path.join(app, 'assets/memes.zip.sha256'), 'utf8').split(/\s+/)[0];
  assert.equal(crypto.createHash('sha256').update(zip).digest('hex'), want);
  const entries = readZip(zip);
  const manifest = JSON.parse(entries.find((e) => e.name === 'memes/manifest.json').data.toString('utf8'));
  const names = new Set(entries.map((e) => e.name));
  for (const meme of manifest.memes) {
    assert.ok(names.has(meme.file) && names.has(meme.still), meme.id);
  }
  assert.equal(entries.length, 1 + new Set(manifest.memes.flatMap((m) => [m.file, m.still])).size);
});
