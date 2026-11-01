#!/usr/bin/env node
// Unpacks the meme pack (Meme mode › High) for Vite: assets/memes.zip → public/memes/.
//   node scripts/memes-unpack.mjs
// The zip must match assets/memes.zip.sha256 and hold only memes/<name>.webp and memes/manifest.json.
// Any problem stops the build (exit 1) without touching public/memes/, so a half pack never ships.
// Node stdlib only: no unzip binary, no dependency.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import zlib from 'node:zlib';

export const LIMITS = { zipBytes: 8 << 20, entries: 64, entryBytes: 1 << 20, totalBytes: 8 << 20 };
const NAME = /^memes\/(?:[a-z0-9-]+(?:\.still)?\.webp|manifest\.json)$/;
const S_IFMT = 0o170000;
const S_IFREG = 0o100000;

const fail = (reason) => {
  throw new Error(reason);
};

/** The entries of a zip, checked: `[{ name, data }]`. Throws on anything outside the rules above. */
export function readZip(buf, limits = LIMITS) {
  if (buf.length > limits.zipBytes) fail(`zip is over ${limits.zipBytes} bytes`);
  // End of central directory: 22 bytes plus a comment of up to 64 KiB.
  let eocd = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 22 - 0xffff); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) fail('not a zip (no end of central directory)');
  if (buf.readUInt16LE(eocd + 4) !== 0 || buf.readUInt16LE(eocd + 6) !== 0) fail('multi-disk zip');
  const count = buf.readUInt16LE(eocd + 10);
  const cdSize = buf.readUInt32LE(eocd + 12);
  const cdStart = buf.readUInt32LE(eocd + 16);
  if (count === 0xffff || cdStart === 0xffffffff) fail('zip64 is not supported');
  if (count > limits.entries) fail(`more than ${limits.entries} entries`);
  if (cdStart + cdSize > eocd) fail('central directory out of range');

  const out = [];
  const seen = new Set();
  let total = 0;
  let p = cdStart;
  for (let n = 0; n < count; n++) {
    if (p + 46 > eocd || buf.readUInt32LE(p) !== 0x02014b50) fail('bad central directory entry');
    const flags = buf.readUInt16LE(p + 8);
    const method = buf.readUInt16LE(p + 10);
    const crc = buf.readUInt32LE(p + 16);
    const packed = buf.readUInt32LE(p + 20);
    const size = buf.readUInt32LE(p + 24);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const commentLen = buf.readUInt16LE(p + 32);
    const madeBy = buf.readUInt16LE(p + 4) >> 8;
    const external = buf.readUInt32LE(p + 38);
    const local = buf.readUInt32LE(p + 42);
    const name = buf.toString('utf8', p + 46, p + 46 + nameLen);
    p += 46 + nameLen + extraLen + commentLen;

    if (!NAME.test(name)) fail(`unexpected entry ${JSON.stringify(name)}`);
    if (seen.has(name)) fail(`duplicate entry ${name}`);
    seen.add(name);
    if (flags & 0x1) fail(`${name} is encrypted`);
    if (method !== 0 && method !== 8) fail(`${name} uses compression method ${method}`);
    // Unix-made entries carry the file mode in the high half: only regular files.
    const mode = external >>> 16;
    if (madeBy === 3 && mode !== 0 && (mode & S_IFMT) !== S_IFREG) fail(`${name} is not a regular file`);
    if (external & 0x10) fail(`${name} is a directory`);
    if (size > limits.entryBytes) fail(`${name} is over ${limits.entryBytes} bytes`);
    total += size;
    if (total > limits.totalBytes) fail(`pack is over ${limits.totalBytes} bytes`);

    if (local + 30 > buf.length || buf.readUInt32LE(local) !== 0x04034b50) fail(`${name}: bad local header`);
    const localNameLen = buf.readUInt16LE(local + 26);
    const localExtraLen = buf.readUInt16LE(local + 28);
    if (buf.toString('utf8', local + 30, local + 30 + localNameLen) !== name) fail(`${name}: local name differs`);
    const start = local + 30 + localNameLen + localExtraLen;
    if (start + packed > cdStart) fail(`${name}: data out of range`);
    const raw = buf.subarray(start, start + packed);
    let data;
    try {
      data = method === 0 ? Buffer.from(raw) : zlib.inflateRawSync(raw, { maxOutputLength: Math.max(size, 1) });
    } catch (err) {
      fail(`${name}: ${err.message}`);
    }
    if (data.length !== size) fail(`${name}: size differs`);
    if (zlib.crc32(data) !== crc) fail(`${name}: CRC mismatch`);
    if (name.endsWith('.webp')) {
      if (size < 12 || data.toString('latin1', 0, 4) !== 'RIFF' || data.toString('latin1', 8, 12) !== 'WEBP') fail(`${name} is not WebP`);
    } else {
      try {
        JSON.parse(data.toString('utf8'));
      } catch {
        fail(`${name} is not JSON`);
      }
    }
    out.push({ name, data });
  }
  if (!seen.has('memes/manifest.json')) fail('memes/manifest.json is missing');
  return out;
}

/** Checks and writes the pack; `'up-to-date'` when the stamp names this zip and every file is in place. */
export function unpack({ zipPath, shaPath, outDir, stampPath, limits = LIMITS }) {
  // A hard-killed earlier run leaves its temp folder, which Vite would copy into dist.
  const parent = path.dirname(outDir);
  const tmpPrefix = `${path.basename(outDir)}.tmp-`;
  if (fs.existsSync(parent)) {
    for (const name of fs.readdirSync(parent)) {
      if (name.startsWith(tmpPrefix)) fs.rmSync(path.join(parent, name), { recursive: true, force: true });
    }
  }
  if (fs.statSync(zipPath).size > limits.zipBytes) fail(`zip is over ${limits.zipBytes} bytes`);
  const buf = fs.readFileSync(zipPath);
  const want = fs.readFileSync(shaPath, 'utf8').trim().split(/\s+/)[0]?.toLowerCase() ?? '';
  const hash = crypto.createHash('sha256').update(buf).digest('hex');
  if (!/^[0-9a-f]{64}$/.test(want)) fail(`${path.basename(shaPath)} holds no SHA-256`);
  if (hash !== want) fail(`SHA-256 of ${path.basename(zipPath)} is ${hash}, expected ${want}`);
  const entries = readZip(buf, limits);
  const stamp = fs.existsSync(stampPath) ? fs.readFileSync(stampPath, 'utf8').trim() : '';
  const inPlace = ({ name, data }) => {
    const file = path.join(outDir, name.slice('memes/'.length));
    return fs.existsSync(file) && fs.statSync(file).size === data.length;
  };
  if (stamp === hash && entries.every(inPlace)) return 'up-to-date';

  const tmp = `${outDir}.tmp-${process.pid}`;
  fs.rmSync(tmp, { recursive: true, force: true });
  try {
    fs.mkdirSync(tmp, { recursive: true });
    for (const { name, data } of entries) fs.writeFileSync(path.join(tmp, name.slice('memes/'.length)), data);
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.renameSync(tmp, outDir);
  } catch (err) {
    fs.rmSync(tmp, { recursive: true, force: true });
    throw err;
  }
  fs.mkdirSync(path.dirname(stampPath), { recursive: true });
  fs.writeFileSync(stampPath, `${hash}\n`);
  return 'unpacked';
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  try {
    const result = unpack({
      zipPath: path.join(app, 'assets/memes.zip'),
      shaPath: path.join(app, 'assets/memes.zip.sha256'),
      outDir: path.join(app, 'public/memes'),
      stampPath: path.join(app, 'node_modules/.cache/anix-memes/stamp'),
    });
    if (result === 'unpacked') console.log('memes-unpack: meme pack unpacked to public/memes/');
  } catch (err) {
    console.error(`memes-unpack: ${err.message}`);
    process.exit(1);
  }
}
