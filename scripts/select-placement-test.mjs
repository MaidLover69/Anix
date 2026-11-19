#!/usr/bin/env node
// #10 §1: where a CustomSelect menu opens (portal, fixed positioning).
//   node --experimental-strip-types scripts/select-placement-test.mjs
import assert from 'node:assert/strict';
import { MENU_GAP, MENU_MARGIN, menuPlacement } from '../src/components/selectPlacement.ts';

assert.equal(MENU_GAP, 4);
assert.equal(MENU_MARGIN, 8);
const base = { viewportWidth: 1100, viewportHeight: 800, menuWidth: 200, menuHeight: 200 };
const trigger = (top, bottom, left = 100, width = 150) => ({ top, bottom, left, width });

// Room below: opens below the trigger, full height.
assert.deepEqual(
  menuPlacement({ ...base, trigger: trigger(100, 130) }),
  { side: 'down', top: 134, left: 100, maxHeight: 200 }
);
// No room below, room above: flips up (top = trigger.top - gap - height).
assert.deepEqual(
  menuPlacement({ ...base, trigger: trigger(670, 700) }),
  { side: 'up', top: 466, left: 100, maxHeight: 200 }
);
// A caller that prefers up gets up when it fits, and down when it does not.
assert.equal(menuPlacement({ ...base, trigger: trigger(400, 430), preferred: 'up' }).side, 'up');
assert.equal(menuPlacement({ ...base, trigger: trigger(100, 130), preferred: 'up' }).side, 'down', 'no room above: flips down');
// Room on neither side: the larger side, the menu shortened (it scrolls) and kept inside the viewport.
const tight = menuPlacement({ ...base, viewportHeight: 400, menuHeight: 500, trigger: trigger(300, 330) });
assert.deepEqual(tight, { side: 'up', top: 8, left: 100, maxHeight: 288 });
const low = menuPlacement({ ...base, viewportHeight: 400, menuHeight: 500, trigger: trigger(50, 80) });
assert.equal(low.side, 'down');
assert.equal(low.top, 84);
assert.equal(low.maxHeight, 400 - 80 - 4 - 8);
// Always inside the viewport, over a sweep of trigger positions.
for (let top = 0; top <= 770; top += 35) {
  const p = menuPlacement({ ...base, menuHeight: 300, trigger: trigger(top, top + 30) });
  const height = Math.min(300, p.maxHeight);
  assert.ok(p.top >= MENU_MARGIN && p.top + height <= 800 - MENU_MARGIN, `inside at ${top}`);
}
// Sideways: kept inside the viewport.
assert.equal(menuPlacement({ ...base, trigger: trigger(100, 130, 1000) }).left, 1100 - 200 - 8);
assert.equal(menuPlacement({ ...base, trigger: trigger(100, 130, -5) }).left, 8);
console.log('select placement tests passed');
