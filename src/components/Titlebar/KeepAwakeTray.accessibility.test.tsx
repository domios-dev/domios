// Run: npx tsx src/components/Titlebar/KeepAwakeTray.accessibility.test.tsx

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(new URL("./KeepAwakeTray.tsx", import.meta.url), "utf8");
const trigger = source.slice(
  source.indexOf('data-testid="keepawake-tray"') - 80,
  source.indexOf('data-testid="keepawake-tray"') + 700,
);

assert.match(trigger, /type="button"/);
assert.match(trigger, /aria-label=\{titleText\}/);
assert.match(trigger, /aria-pressed=\{status\.active\}/);
assert.equal(
  (trigger.match(/ct-liquid-interactive/g) ?? []).length,
  1,
  "active Keep Awake state uses the shared interaction contract",
);
assert.match(trigger, /: UI_TRAY_BTN/);

console.log("Keep Awake tray accessibility tests passed");
