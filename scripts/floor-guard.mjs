#!/usr/bin/env node
// Floor guard: checks a diff for moves that lower the quality bar in CONSTRAINTS.md.
//   node scripts/floor-guard.mjs [--base <ref>]     (default base: origin/main)
// Exit 0 clean, 1 at least one floor violation, 2 the guard could not run (never read as clean).
// Looks at added and removed lines from the merge base to the working tree, untracked files included.
import { execFileSync } from 'node:child_process';

const base = (() => {
  const i = process.argv.indexOf('--base');
  return i > -1 ? process.argv[i + 1] : 'origin/main';
})();

// `git diff --no-index` exits 1 when the sides differ (always, for a new file): keep that output.
const git = (args, { diffExit = false } = {}) => {
  try { return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 256 << 20 }); }
  catch (e) { return diffExit && e.status === 1 && typeof e.stdout === 'string' ? e.stdout : null; }
};
const bail = (msg) => { console.error('floor-guard: ' + msg); process.exit(2); };

const top = git(['rev-parse', '--show-toplevel'])?.trim();
if (!top) bail('not inside a git work tree');
process.chdir(top);
const mergeBase = git(['merge-base', base, 'HEAD'])?.trim();
if (!mergeBase) bail('no merge base against ' + base);

const tracked = git(['diff', '--unified=0', mergeBase, '--']);
if (tracked === null) bail('could not diff against ' + mergeBase);
const untrackedList = git(['ls-files', '--others', '--exclude-standard']);
if (untrackedList === null) bail('could not list untracked files');
const untracked = untrackedList.split('\n').filter(Boolean).map((f) => {
  const d = git(['diff', '--no-index', '--unified=0', '/dev/null', f], { diffExit: true });
  if (d === null) bail('could not diff untracked file ' + f);
  return d;
}).join('\n');

const added = [], removed = [], deleted = [];
const pathOf = (s) => s.replace(/^[ab]\//, '');
let file = '', oldFile = '', inHeader = false;
for (const line of (tracked + '\n' + untracked).split('\n')) {
  if (line.startsWith('diff ')) inHeader = true;
  else if (line.startsWith('@@')) inHeader = false;
  else if (inHeader) {
    if (line.startsWith('--- ')) oldFile = pathOf(line.slice(4));
    else if (line.startsWith('+++ ')) {
      const newFile = pathOf(line.slice(4));
      file = newFile === '/dev/null' ? oldFile : newFile;
      if (newFile === '/dev/null') deleted.push(file);
    }
  }
  else if (line.startsWith('+')) added.push({ file, text: line.slice(1) });
  else if (line.startsWith('-')) removed.push({ file, text: line.slice(1) });
}

const findings = [];
const flag = (rule, f, text) => findings.push({ rule, file: f, text: text.trim().slice(0, 120) });
const isTest = (f) => /(^|\/)tests\/|_tests?\.rs$|-test\.mjs$|\.(test|spec)\.[jt]sx?$/.test(f);
const isConstraints = (f) => /(^|\/)CONSTRAINTS\.md$/.test(f);
const isSelf = (f) => /scripts\/floor-guard(-test)?\.mjs$/.test(f); // the patterns themselves
const isCode = (f) => /\.(rs|[cm]?[jt]sx?)$/.test(f); // docs may name the patterns

// A silenced checker. `#[allow(...)]` is fine with a reason on the same line: `#[allow(x)] // why`.
const SUPPRESSIONS = /@ts-ignore|@ts-nocheck|@ts-expect-error|eslint-disable|biome-ignore|istanbul ignore|nosemgrep|gitleaks:allow/;
const ALLOW_NO_REASON = /#!?\[allow\([^)]*\)\](?!.*\/\/\s*\S)/;
// Unfinished work.
const STUBS = /\btodo!\(|\bunimplemented!\(|throw new Error\([^)]*[Nn]ot implemented|catch\s*(\(\s*\w*\s*\))?\s*\{\s*\}|\bTODO\b/;
// A test made easier.
const SKIPS = /#\[ignore\b|\.(skip|only)\(|\bxit\(|\bxdescribe\(|\btest\.todo\(/;
const ASSERT = /\bassert(_eq|_ne|_matches)?!|\bassert(\.\w+)?\(|(?<!\.)\bexpect\(/;

for (const { file, text } of added) {
  if (isConstraints(file) && /^\| *[WE]\d+ *\|/.test(text)) flag('new-exception', file, text);
  if (isSelf(file) || !isCode(file)) continue;
  if (SUPPRESSIONS.test(text) || ALLOW_NO_REASON.test(text)) flag('silenced-checker', file, text);
  if (STUBS.test(text)) flag('unfinished-work', file, text);
  if (SKIPS.test(text)) flag('test-made-easier', file, text);
}
for (const f of deleted) if (isTest(f)) flag('test-deleted', f, 'file deleted');
// An edited assertion shows up as one removed and one added line, so flag only a net loss per test file.
const assertCount = {};
for (const { file, text } of removed) {
  if (isTest(file) && !deleted.includes(file) && ASSERT.test(text)) (assertCount[file] ??= { lost: [], added: 0 }).lost.push(text);
}
for (const { file, text } of added) if (assertCount[file] && ASSERT.test(text)) assertCount[file].added++;
for (const [f, { lost, added: n }] of Object.entries(assertCount)) {
  if (lost.length > n) flag('assertion-removed', f, `${lost.length - n} fewer assertion(s), e.g. ${lost[0].trim()}`);
}

// A rule in CONSTRAINTS.md weakened or removed. A rule is a floor bullet (text before its first colon)
// or a table row (first cell). Each number gets a direction from the words around it; a number with no
// readable direction is reported whenever it changes.
const ruleKey = (t) => {
  const s = t.trim();
  if (s.startsWith('|')) return s.split('|').map((c) => c.trim()).filter(Boolean)[0] ?? '';
  if (/^[-*] /.test(s)) return s.slice(2).split(':')[0].trim();
  return null;
};
const isException = (t) => /^\| *[WE]\d+ *\|/.test(t.trim());
const MIN_BEFORE = /(>=|>|≥|at least|minimum|\bmin\b|no less than|not fall|not drop)\s*$/;
const MAX_BEFORE = /(<=|<|≤|at most|maximum|\bmax\b|no more than|under|below|not grow|not exceed)\s*$/;
const MIN_AFTER = /^\s*\S*\s*(or more|or higher|must not fall|must not drop)/;
const MAX_AFTER = /^\s*\S*\s*(or less|or lower|must not grow|must not exceed)/;
const thresholds = (t) => {
  const out = [], re = /\d+(?:\.\d+)?/g;
  let m;
  while ((m = re.exec(t))) {
    const before = t.slice(Math.max(0, m.index - 24), m.index).toLowerCase();
    const after = t.slice(m.index + m[0].length, m.index + m[0].length + 40).toLowerCase();
    const dir = MIN_BEFORE.test(before) || MIN_AFTER.test(after) ? 'min'
      : MAX_BEFORE.test(before) || MAX_AFTER.test(after) ? 'max' : null;
    out.push({ n: Number(m[0]), dir });
  }
  return out;
};
const removedRules = removed.filter((l) => isConstraints(l.file) && ruleKey(l.text) !== null);
const addedRules = added.filter((l) => isConstraints(l.file) && ruleKey(l.text) !== null);
for (const r of removedRules) {
  const a = addedRules.find((x) => ruleKey(x.text) === ruleKey(r.text));
  if (!a) {
    if (!isException(r.text)) flag('rule-removed', r.file, r.text);
    continue;
  }
  const before = thresholds(r.text), after = thresholds(a.text);
  let verdict = null;
  for (const dir of ['min', 'max', null]) {
    const was = before.filter((x) => x.dir === dir), now = after.filter((x) => x.dir === dir);
    was.forEach((b, i) => {
      const n = now[i];
      if (verdict) return;
      if (!n) verdict = 'threshold-removed';
      else if (n.n === b.n) return;
      else if (dir === 'min' ? n.n < b.n : dir === 'max' ? n.n > b.n : true) {
        verdict = dir ? 'threshold-loosened' : 'threshold-changed';
      }
    });
  }
  if (verdict) flag(verdict, r.file, r.text + '  ->  ' + a.text);
}

if (findings.length === 0) { console.log('floor-guard: clean'); process.exit(0); }
console.error(`floor-guard: ${findings.length} floor violation(s):`);
for (const f of findings) console.error(`  [${f.rule}] ${f.file}: ${f.text}`);
console.error('\nEach one lowers the bar in CONSTRAINTS.md. Fix the code, or add a dated exception there with a reason.');
process.exit(1);
