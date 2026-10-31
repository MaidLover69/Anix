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
  'CONSTRAINTS.md': '# Constraints\n\n| Metric | Today | Direction |\n|---|---|---|\n| Main JS chunk | 1421898 bytes, must not grow | down |\n'
    + '| Idle CPU on Home | at most 1 % (measured ≈0 %) | down |\n',
};
const gitIn = (dir, ...args) => execFileSync('git', args, { cwd: dir, stdio: 'ignore' });

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

test('a new measured note passes, a looser limit in the same row does not', () => {
  let d = repo(BASE);
  write(d, { 'CONSTRAINTS.md': BASE['CONSTRAINTS.md'].replace('(measured ≈0 %)', '(measured 9–12 %: the banner; 0 % paused), must fall') });
  assert.equal(guard(d).code, 0);
  d = repo(BASE);
  write(d, { 'CONSTRAINTS.md': BASE['CONSTRAINTS.md'].replace('at most 1 %', 'at most 2 %') });
  assert.match(guard(d).out, /threshold-loosened/);
});

test('a dated exception row lets its rule through and is printed; a malformed one fails', () => {
  const loose = BASE['CONSTRAINTS.md'].replace('at most 1 %', 'at most 2 %');
  let d = repo(BASE);
  write(d, { 'CONSTRAINTS.md': loose + '| E1 | 2026-10-31 | threshold-loosened | banner animation until 1.5 | owner |\n' });
  let r = guard(d);
  assert.equal(r.code, 0);
  assert.match(r.out, /new exception \| E1/);
  assert.match(r.out, /excused \[threshold-loosened\]/);
  d = repo(BASE);
  write(d, { 'CONSTRAINTS.md': loose + '| E1 | soon | threshold-loosened | | |\n' });
  r = guard(d);
  assert.equal(r.code, 1);
  assert.match(r.out, /bad-exception/);
  d = repo(BASE);
  write(d, { 'CONSTRAINTS.md': loose + '| E1 | 2026-10-31 | silenced-checker | other rule | owner |\n' });
  assert.match(guard(d).out, /floor violation.*\n.*threshold-loosened/);
});

test('a network test may be ignored when it says so; a deleted fixture is not a deleted test', () => {
  const d = repo({ ...BASE, 'src-tauri/tests/fixtures/page.html': '<p></p>\n' });
  write(d, {
    'src-tauri/tests/a_tests.rs': BASE['src-tauri/tests/a_tests.rs'] + '#[test]\n#[ignore = "needs network"]\nfn live() {\n    assert!(true);\n}\n',
    'src-tauri/tests/fixtures/page.html': null,
  });
  assert.equal(guard(d).code, 0);
});

test('renaming a test file out of the harness is a deleted test; a rename inside it is not', () => {
  for (const to of ['src-tauri/tests/a_tests.rs.off', 'src-tauri/tests/old/a_tests.rs']) {
    const d = repo(BASE);
    mkdirSync(join(d, 'src-tauri/tests/old'), { recursive: true });
    gitIn(d, 'mv', 'src-tauri/tests/a_tests.rs', to);
    assert.match(guard(d).out, /test-deleted.*a_tests\.rs: renamed to/);
  }
  const d = repo(BASE);
  gitIn(d, 'mv', 'src-tauri/tests/a_tests.rs', 'src-tauri/tests/b_tests.rs');
  assert.equal(guard(d).code, 0);
});

test('files with a space or non-ASCII characters in the path are checked', () => {
  const d = repo({ ...BASE, 'src/my file.ts': 'export const b = 1;\n', 'src/é.ts': 'export const c = 1;\n' });
  write(d, {
    'src/my file.ts': '// @ts-ignore\nexport const b = 1;\n', // tracked, header ends in a TAB
    'src/é.ts': '// @ts-ignore\nexport const c = 1;\n', // tracked, quoted by default
    'src/new file ü.ts': '// @ts-ignore\n', // untracked, listed through ls-files -z
  });
  const r = guard(d);
  assert.equal((r.out.match(/silenced-checker/g) || []).length, 3, r.out);
});

test('expect, cfg_attr allow and a Cargo.toml allow need a reason; reason = counts as one', () => {
  let d = repo({ ...BASE, 'src-tauri/Cargo.toml': '[package]\nname = "a"\n' });
  write(d, {
    'src-tauri/src/lib.rs': '#[expect(dead_code)]\n#[cfg_attr(test, allow(unused))]\n#[allow(\n    clippy::x,\n)]\npub fn a() {}\n',
    'src-tauri/Cargo.toml': '[package]\nname = "a"\n\n[lints.clippy]\ntoo_many_arguments = "allow"\ny = { level = "allow", priority = -1 }\n',
    'src-tauri/tests/a_tests.rs': BASE['src-tauri/tests/a_tests.rs'] + '#[test]\n#[ignore] // network is flaky\nfn t2() {}\n',
  });
  const r = guard(d).out;
  assert.equal((r.match(/silenced-checker/g) || []).length, 5, r);
  assert.match(r, /test-made-easier/);
  d = repo({ ...BASE, 'src-tauri/Cargo.toml': '[package]\nname = "a"\n' });
  write(d, {
    'src-tauri/src/lib.rs': '#[allow(clippy::too_many_arguments, reason = "mirrors the IPC call")]\n#[allow(\n    clippy::x,\n    reason = "split by rustfmt",\n)]\npub fn a() {}\n',
    'src-tauri/Cargo.toml': '[package]\nname = "a"\n\n[lints.clippy]\n# commands mirror the UI call\ntoo_many_arguments = "allow"\nx = "allow" # why\n',
  });
  assert.equal(guard(d).code, 0);
});
