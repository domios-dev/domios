// Run: npx tsx src/store/multiplexer/tree.test.ts

import { displayTitleForTab } from "./tree";
import type { Tab, Tab } from "../../lib/types";

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

const tab: Tab = { id: "tab-1", tab_id: "abcdefgh1234" };

test("displayTitleForTab uses tab.title when present", () => {
  const tab = { id: "abcdefgh1234", title: "  Puccini  " } as Tab;
  assert(displayTitleForTab(tab, tab) === "Puccini", "trimmed Tab title");
});

test("displayTitleForTab falls back to tab_id prefix when tab title is empty", () => {
  const tab = { id: "abcdefgh1234", title: "   " } as Tab;
  assert(displayTitleForTab(tab, tab) === "abcdefgh", "tab_id prefix fallback");
});

test("displayTitleForTab falls back to tab.tab_id prefix when tab is missing", () => {
  assert(displayTitleForTab(tab, null) === "abcdefgh", "missing tab uses tab.tab_id");
});

console.log(failures === 0 ? "✓ tree tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
