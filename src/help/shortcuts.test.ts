// Run: npx tsx src/help/shortcuts.test.ts

import { resolveHelpShortcut } from "./shortcuts";
import type { AppConfig, KeybindingsConfig } from "../lib/types";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const keybindings: KeybindingsConfig = { reopen_tab: "Ctrl+Shift+T" };
const config = { keybindings } as AppConfig;

test("resolves app shortcuts from registry", () => {
  assert(
    resolveHelpShortcut({ kind: "app", id: "command_palette" }, config) === "Ctrl+K",
    "command palette binding",
  );
});

test("resolves config keybindings from live config", () => {
  assert(
    resolveHelpShortcut({ kind: "config", key: "reopen_tab" }, config) === "Ctrl+Shift+T",
    "reopen_tab from config",
  );
});

test("falls back to empty string for a missing optional binding", () => {
  assert(
    resolveHelpShortcut({ kind: "config", key: "search" }, config) === "",
    "missing optional binding falls back to empty string",
  );
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : e}`);
  }
}
console.log(`help/shortcuts tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
process.exit(failed === 0 ? 0 : 1);
