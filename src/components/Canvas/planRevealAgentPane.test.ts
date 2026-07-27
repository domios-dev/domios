// Run: npx tsx src/components/Canvas/planRevealAgentPane.test.ts

import type { Workspace } from "../../lib/types";
import { canvasFocusStore } from "./canvasFocusStore";
import { findPaneTabContext, revealAssignedAgentPane } from "./planRevealAgentPane";

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
  active_tab_id: "tab-1",
  tabs: [
    { id: "tab-1", pane_id: "orch-1" },
    { id: "tab-2", pane_id: "worker-1" },
  ],
};

test("findPaneTabContext resolves tab for Tab id", () => {
  const ctx = findPaneTabContext([workspace], "worker-1");
  assert(ctx?.workspaceId === "ws-1", "workspace id");
  assert(ctx?.tabId === "tab-2", "tab id");
});

test("findPaneTabContext trims Tab id", () => {
  const ctx = findPaneTabContext([workspace], " worker-1 ");
  assert(ctx?.tabId === "tab-2", "trimmed Tab resolves");
});

test("findPaneTabContext returns null for unknown Tab", () => {
  assert(findPaneTabContext([workspace], "missing") === null, "unknown Tab");
  assert(findPaneTabContext([workspace], "  ") === null, "blank Tab");
});

test("revealAssignedAgentPane closes canvas overlays and activates tab", () => {
  canvasFocusStore.openFocus({ planId: "pl-1", focusPaneId: "orch-1" });
  canvasFocusStore.openWorkspace({ planId: "pl-1" });

  const activeTabs: Array<{ workspaceId: string; tabId: string }> = [];
  let focused: string | null = null;

  const ok = revealAssignedAgentPane(
    {
      workspaces: [workspace],
      setActivepane: (workspaceId, tabId) => {
        activeTabs.push({ workspaceId, tabId });
      },
      setFocusedPane: (paneId) => {
        focused = paneId;
      },
    },
    "worker-1",
  );

  assert(ok, "revealed");
  assert(!canvasFocusStore.isOpen(), "focus canvas closed");
  assert(!canvasFocusStore.isWorkspaceOpen(), "workspace canvas closed");
  const activeTab = activeTabs[0];
  assert(activeTab, "tab activated once");
  assert(activeTab?.workspaceId === "ws-1" && activeTab?.tabId === "tab-2", "tab activated");
  assert(focused === "worker-1", "Tab focused");
});

test("revealAssignedAgentPane still focuses when tab context is missing", () => {
  let focused: string | null = null;
  const ok = revealAssignedAgentPane(
    {
      workspaces: [],
      setActivepane: () => {},
      setFocusedPane: (paneId) => {
        focused = paneId;
      },
    },
    "orphan-pane",
  );
  assert(ok, "revealed without tab");
  assert(focused === "orphan-pane", "Tab still focused");
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
  console.log(`\n✓ ${passed}/${tests.length} planRevealAgentPane tests passed`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
