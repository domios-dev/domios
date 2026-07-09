// Run: npx tsx src/components/Sidebar/sidebarSelection.test.ts

import { isWorkspacePlanEntryActive, isWorkspaceTabActive, workspacePlanTabId } from "./sidebarSelection";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("plan active only when workspace is visual and canvas is open", () => {
  assert(!isWorkspacePlanEntryActive(false, false), "inactive workspace");
  assert(!isWorkspacePlanEntryActive(false, true), "inactive workspace even if canvas flag set");
  assert(!isWorkspacePlanEntryActive(true, false), "workspace mode but canvas closed");
  assert(isWorkspacePlanEntryActive(true, true), "workspace mode with canvas open");
});

test("workspace plan tab id is stable and workspace-scoped", () => {
  assert(workspacePlanTabId("ws-1") === "workspace-plan:ws-1", "uses virtual workspace plan namespace");
  assert(workspacePlanTabId("workspace_abc") === "workspace-plan:workspace_abc", "does not derive from pane/tab ids");
});

test("tab active only when workspace visual, canvas closed, and tab matches", () => {
  assert(isWorkspaceTabActive(true, false, "tab-a", "tab-a"), "matching tab");
  assert(!isWorkspaceTabActive(true, false, "tab-a", "tab-b"), "different tab");
  assert(!isWorkspaceTabActive(true, true, "tab-a", "tab-a"), "canvas open suppresses tab highlight");
  assert(!isWorkspaceTabActive(false, false, "tab-a", "tab-a"), "inactive workspace");
});

test("plan and tab highlights are mutually exclusive for the same workspace", () => {
  const cases: Array<[boolean, boolean, string, string | undefined]> = [
    [true, true, "tab-1", "tab-1"],
    [true, true, "tab-1", "tab-2"],
    [true, false, "tab-1", "tab-1"],
    [true, false, "tab-1", undefined],
    [false, true, "tab-1", "tab-1"],
  ];
  for (const [visual, canvas, tabId, activeTabId] of cases) {
    const plan = isWorkspacePlanEntryActive(visual, canvas);
    const tab = isWorkspaceTabActive(visual, canvas, tabId, activeTabId);
    assert(!(plan && tab), `plan+tab both active: visual=${visual} canvas=${canvas} tab=${tabId} active=${activeTabId}`);
  }
});

function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

main();
