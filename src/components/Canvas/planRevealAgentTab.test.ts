// Run: npx tsx src/components/Canvas/planRevealAgentTab.test.ts

import type { Workspace } from "../../lib/types";
import { canvasFocusStore } from "./canvasFocusStore";
import { findTabContext, revealAssignedAgentTab } from "./planRevealAgentTab";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const workspace: Workspace = {
  id: "ws-1",
  name: "Hub",
  cwd: "/repo/hub",
  active_tab_id: "orch-1",
  tabs: ["orch-1", "worker-1"],
};

test("findTabContext resolves workspace for Tab id", () => {
  const ctx = findTabContext([workspace], "worker-1");
  assert(ctx?.workspaceId === "ws-1", "workspace id");
  assert(ctx?.tabId === "worker-1", "tab id");
});

test("findTabContext trims Tab id", () => {
  const ctx = findTabContext([workspace], " worker-1 ");
  assert(ctx?.tabId === "worker-1", "trimmed Tab resolves");
});

test("findTabContext returns null for unknown Tab", () => {
  assert(findTabContext([workspace], "missing") === null, "unknown Tab");
  assert(findTabContext([workspace], "  ") === null, "blank Tab");
});

test("revealAssignedAgentTab closes canvas overlays and activates tab", () => {
  canvasFocusStore.openFocus({ planId: "pl-1", focusTabId: "orch-1" });
  canvasFocusStore.openWorkspace({ planId: "pl-1" });

  const activeTabs: Array<{ workspaceId: string; tabId: string }> = [];
  const focusedTabs: string[] = [];

  const ok = revealAssignedAgentTab(
    {
      workspaces: [workspace],
      setActiveTab: (workspaceId, tabId) => {
        activeTabs.push({ workspaceId, tabId });
      },
      setFocusedTab: (tabId) => {
        focusedTabs.push(tabId);
      },
    },
    "worker-1",
  );

  assert(ok, "revealed");
  assert(!canvasFocusStore.isOpen(), "focus canvas closed");
  assert(!canvasFocusStore.isWorkspaceOpen(), "workspace canvas closed");
  const activeTab = activeTabs[0];
  assert(activeTabs.length === 1, `tab activated ${activeTabs.length} times`);
  assert(activeTab?.workspaceId === "ws-1" && activeTab?.tabId === "worker-1", "tab activated");
  assert(focusedTabs.length === 1, `Tab focused ${focusedTabs.length} times`);
  assert(focusedTabs[0] === "worker-1", "target Tab focused");
});

test("revealAssignedAgentTab still focuses when tab context is missing", () => {
  let focused: string | null = null;
  const ok = revealAssignedAgentTab(
    {
      workspaces: [],
      setActiveTab: () => {},
      setFocusedTab: (tabId) => {
        focused = tabId;
      },
    },
    "orphan-tab",
  );
  assert(ok, "revealed without tab");
  assert(focused === "orphan-tab", "Tab still focused");
});

async function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} planRevealAgentTab tests passed`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
