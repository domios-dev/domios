// Run: npx tsx src/components/Sidebar/hierarchyRuntimeClose.test.ts
// Runtime-close sibling of hierarchyTabGone.test.ts (structural derivation).

import { reducer } from "../../store/multiplexer/reducer";
import type { ExtendedState } from "../../store/multiplexer/tree";
import type { MultiplexerState, Tab, TabStatus } from "../../lib/types";
import { buildHierarchySubtree } from "./HierarchyTree";

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

function tab(id: string, role: Tab["role"], parent?: string): Tab {
  return {
    id,
    pty_id: id,
    tab_type: "Terminal",
    title: id,
    cwd: null,
    role,
    parent_tab_id: parent ?? null,
  } as Tab;
}

function base(): ExtendedState {
  return {
    workspaces: [],
    tabs: {},
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    pendingCloses: [],
  };
}

function snapshot(
  generation: number,
  tabs: Record<string, Tab>,
): MultiplexerState {
  return {
    workspaces: [
      { id: "ws", name: "W", tabs: [], active_tab_id: null, cwd: null } as never,
    ],
    tabs,
    active_workspace_id: "ws",
    focused_tab_id: null,
    generation,
  };
}

const orch = { ...tab("orch", "Manager"), role_profile: "orchestrator" };
const liveWorker = tab("worker-live", "Worker", "orch");
const goneWorker = tab("worker-gone", "Worker", "orch");
const statuses: Record<string, TabStatus> = {
  "worker-live": "Working",
  "worker-gone": "Working",
};

function subtreeIds(tabs: Record<string, Tab>): string[] {
  const nodes = buildHierarchySubtree("orch", tabs, statuses);
  const walk = (ns: typeof nodes): string[] =>
    ns.flatMap((n) => [n.tab.id, ...walk(n.children)]);
  return walk(nodes);
}

// A backend snapshot with both workers → both render under the orchestrator.
const withBoth = reducer(base(), {
  type: "SET_STATE",
  state: snapshot(1, { orch, "worker-live": liveWorker, "worker-gone": goneWorker }),
});

test("both workers render before the runtime close", () => {
  const ids = subtreeIds(withBoth.tabs);
  assert(ids.includes("worker-live"), "live worker present");
  assert(ids.includes("worker-gone"), "gone worker present pre-close");
});

// Runtime close: a fresh higher-generation snapshot prunes worker-gone; the
// reducer must drop it from state.tabs.
const afterClose = reducer(withBoth, {
  type: "SET_STATE",
  state: snapshot(2, { orch, "worker-live": liveWorker }),
});

test("closed worker leaves state.tabs on the next snapshot", () => {
  assert(!afterClose.tabs["worker-gone"], "worker-gone pruned from state.tabs");
  assert(afterClose.tabs["worker-live"], "live worker retained");
});

test("closed worker is not rendered as a live child after the close", () => {
  const ids = subtreeIds(afterClose.tabs);
  assert(ids.includes("worker-live"), "live worker still rendered");
  assert(!ids.includes("worker-gone"), "closed worker must NOT render as a live child");
});

// Poisoned fixture: a stale status entry for worker-gone (missed Dead event) must
// not resurrect a row — rows derive from state.tabs membership, not the status map.
test("stale status entry for a gone Tab does not resurrect a row", () => {
  assert(statuses["worker-gone"] === "Working", "fixture keeps the stale status entry");
  const nodes = buildHierarchySubtree("orch", afterClose.tabs, statuses);
  const ids = nodes.flatMap((n) => [n.tab.id, ...n.children.map((c) => c.tab.id)]);
  assert(!ids.includes("worker-gone"), "stale status fixture must not add a ghost row");
});

if (failures > 0) {
  console.error(`\n${failures} test(s) failed`);
  process.exit(1);
}
console.log("\n✓ hierarchy runtime-close: pruned worker never renders as a ghost row");
