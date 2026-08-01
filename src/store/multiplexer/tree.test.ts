// Run: npx tsx src/store/multiplexer/tree.test.ts

import { displayTitleForTab } from "./tree";
import type { Tab } from "../../lib/types";

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

const tab = { id: "abcdefgh1234", title: "" } as Tab;

test("displayTitleForTab uses tab.title when present", () => {
  const tab = { id: "abcdefgh1234", title: "  Puccini  " } as Tab;
  assert(displayTitleForTab(tab) === "Puccini", "trimmed Tab title");
});

test("displayTitleForTab falls back to tab_id prefix when tab title is empty", () => {
  const tab = { id: "abcdefgh1234", title: "   " } as Tab;
  assert(displayTitleForTab(tab) === "abcdefgh", "Tab id prefix fallback");
});

test("displayTitleForTab falls back to Tab id prefix", () => {
  assert(displayTitleForTab(tab) === "abcdefgh", "empty title uses Tab id");
});

console.log(failures === 0 ? "✓ tree tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
