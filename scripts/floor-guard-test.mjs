// Tests for scripts/floor-guard.mjs: run with `node --test scripts/floor-guard-test.mjs`.
// Each case builds a throwaway git repo, commits a base, makes a change and runs the guard against it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const GUARD = join(dirname(fileURLToPath(import.meta.url)), 'floor-guard.mjs');

function repo(files) {
  const dir = mkdtempSync(join(tmpdir(), 'floor-guard-'));
  const g = (...a) => execFileSync('git', a, { cwd: dir, stdio: 'ignore' });
  g('init', '-q', '-b', 'main');
  g('config', 'user.email', 't@example.invalid'); g('config', 'user.name', 't'); g('config', 'commit.gpgsign', 'false');
  write(dir, files);
  g('add', '-A'); g('commit', '-q', '-m', 'base');
  return dir;
}
function write(dir, files) {
  for (const [f, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, f)), { recursive: true });
    if (text === null) rmSync(join(dir, f)); else writeFileSync(join(dir, f), text);
  }
}
function guard(dir) {
  const r = spawnSync('node', [GUARD, '--base', 'main'], { cwd: dir, encoding: 'utf8' });
  rmSync(dir, { recursive: true, force: true });
  return { code: r.status, out: r.stdout + r.stderr };
}

const BASE = {
  'src/a.ts': 'export const a = 1;\n',
  'src-tauri/src/lib.rs': 'pub fn a() {}\n',
  'src-tauri/tests/a_tests.rs': '#[test]\nfn t() {\n    assert_eq!(1, 1);\n}\n',
  'CONSTRAINTS.md': '# Constraints\n\n| Metric | Today | Direction |\n|---|---|---|\n| Main JS chunk | 1421898 bytes, must not grow | down |\n',
};

test('a clean change passes', () => {
  const d = repo(BASE);
  write(d, { 'src/a.ts': 'export const a = 2;\n', 'src-tauri/src/lib.rs': '#[allow(dead_code)] // kept for the C API\npub fn a() {}\n' });
  assert.equal(guard(d).code, 0);
});

test('suppressions and allow without a reason are flagged', () => {
  const d = repo(BASE);
  write(d, { 'src/a.ts': '// @ts-ignore\nexport const a = 1;\n', 'src-tauri/src/lib.rs': '#[allow(dead_code)]\npub fn a() {}\n' });
  const r = guard(d);
  assert.equal(r.code, 1);
  assert.equal((r.out.match(/silenced-checker/g) || []).length, 2);
});

test('docs that name the patterns are not flagged', () => {
  const d = repo(BASE);
  write(d, { 'CONSTRAINTS.md': BASE['CONSTRAINTS.md'] + '\n- No new `@ts-ignore`, `todo!()` or `#[ignore]`.\n', 'README.md': 'Avoid `catch {}`.\n' });
  assert.equal(guard(d).code, 0);
});

test('stubs, empty catch and skipped tests are flagged', () => {
  const d = repo(BASE);
  write(d, {
    'src/b.ts': 'try { x() } catch {}\n',
    'src-tauri/src/lib.rs': 'pub fn a() { todo!() }\n',
    'src-tauri/tests/a_tests.rs': '#[test]\n#[ignore]\nfn t() {\n    assert_eq!(1, 1);\n}\n',
  });
  const r = guard(d);
  assert.equal(r.code, 1);
  assert.match(r.out, /unfinished-work.*src\/b\.ts/);
  assert.match(r.out, /unfinished-work.*lib\.rs/);
  assert.match(r.out, /test-made-easier/);
});

test('a removed assertion and a deleted test file are flagged', () => {
  let d = repo(BASE);
  write(d, { 'src-tauri/tests/a_tests.rs': '#[test]\nfn t() {\n}\n' });
  assert.match(guard(d).out, /assertion-removed/);
  d = repo(BASE);
  write(d, { 'src-tauri/tests/a_tests.rs': null });
  assert.match(guard(d).out, /test-deleted/);
});

test('an edited assertion is not a removed one', () => {
  const d = repo(BASE);
  write(d, { 'src-tauri/tests/a_tests.rs': '#[test]\nfn t() {\n    assert_eq!(2, 2);\n}\n' });
  assert.equal(guard(d).code, 0);
});

test('a loosened ratchet is flagged, a tightened one is not', () => {
  let d = repo(BASE);
  write(d, { 'CONSTRAINTS.md': BASE['CONSTRAINTS.md'].replace('1421898', '1500000') });
  assert.match(guard(d).out, /threshold-loosened/);
  d = repo(BASE);
  write(d, { 'CONSTRAINTS.md': BASE['CONSTRAINTS.md'].replace('1421898', '900000') });
  assert.equal(guard(d).code, 0);
});

test('no merge base is exit 2, never clean', () => {
  const d = repo(BASE);
  const r = spawnSync('node', [GUARD, '--base', 'no-such-ref'], { cwd: d, encoding: 'utf8' });
  rmSync(d, { recursive: true, force: true });
  assert.equal(r.status, 2);
});
