#!/usr/bin/env node
// Tests for where WebKit's own right-click menu is kept (text fields, selected text) and hidden.
//   node --experimental-strip-types scripts/page-menu-test.mjs
import assert from 'node:assert/strict';
import { keepsPageMenu } from '../src/lib/pageMenu.ts';

// A stand-in element: `closest` answers for the one selector the app asks about.
const el = (inField) => ({ closest: () => (inField ? {} : null) });

assert.equal(keepsPageMenu(el(false), ''), false, 'empty spot: hidden');
assert.equal(keepsPageMenu(el(false), '   \n'), false, 'blank selection: hidden');
assert.equal(keepsPageMenu(el(true), ''), true, 'text field: kept');
assert.equal(keepsPageMenu(el(false), 'Frieren'), true, 'selected text: kept');
assert.equal(keepsPageMenu(null, ''), false, 'no target: hidden');
assert.equal(keepsPageMenu({}, ''), false, 'target without closest (document, text node): hidden');
console.log('page-menu-test: ok');
