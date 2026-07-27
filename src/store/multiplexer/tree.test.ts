// Run: npx tsx src/store/multiplexer/tree.test.ts

import { displayTitleForTab } from "./tree";
import type { Pane, Tab } from "../../lib/types";

let failures = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failures++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

const tab: Tab = { id: "tab-1", pane_id: "abcdefgh1234" };

test("displayTitleForTab uses pane.title when present", () => {
  const pane = { id: "abcdefgh1234", title: "  Puccini  " } as Pane;
  assert(displayTitleForTab(tab, pane) === "Puccini", "trimmed Tab title");
});

test("displayTitleForTab falls back to pane_id prefix when pane title is empty", () => {
  const pane = { id: "abcdefgh1234", title: "   " } as Pane;
  assert(displayTitleForTab(tab, pane) === "abcdefgh", "pane_id prefix fallback");
});

test("displayTitleForTab falls back to tab.pane_id prefix when pane is missing", () => {
  assert(displayTitleForTab(tab, null) === "abcdefgh", "missing pane uses tab.pane_id");
});

console.log(failures === 0 ? "✓ tree tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
