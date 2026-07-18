// Run: npx tsx src/components/Sidebar/hierarchyRuntimeClose.test.ts
// Runtime-close sibling of hierarchyPaneGone.test.ts (structural derivation).

import { reducer } from "../../store/multiplexer/reducer";
import type { ExtendedState } from "../../store/multiplexer/tree";
import type { MultiplexerState, Pane, PaneStatus } from "../../lib/types";
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

function pane(id: string, role: Pane["role"], parent?: string): Pane {
  return {
    id,
    pty_id: id,
    pane_type: "Terminal",
    title: id,
    cwd: null,
    role,
    parent_pane_id: parent ?? null,
  } as Pane;
}

function base(): ExtendedState {
  return {
    workspaces: [],
    panes: {},
    teams: [],
    active_workspace_id: null,
    focused_pane_id: null,
    notifications: [],
    paneStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
  };
}

function snapshot(
  generation: number,
  panes: Record<string, Pane>,
): MultiplexerState {
  return {
    workspaces: [
      { id: "ws", name: "W", tabs: [], active_tab_id: null, cwd: null } as never,
    ],
    panes,
    active_workspace_id: "ws",
    focused_pane_id: null,
    generation,
  };
}

const orch = pane("orch", "Orchestrator");
const liveWorker = pane("worker-live", "Worker", "orch");
const goneWorker = pane("worker-gone", "Worker", "orch");
const statuses: Record<string, PaneStatus> = {
  "worker-live": "Working",
  "worker-gone": "Working",
};

function subtreeIds(panes: Record<string, Pane>): string[] {
  const nodes = buildHierarchySubtree("orch", panes, statuses);
  const walk = (ns: typeof nodes): string[] =>
    ns.flatMap((n) => [n.pane.id, ...walk(n.children)]);
  return walk(nodes);
}

// A backend snapshot with both workers → both render under the orchestrator.
const withBoth = reducer(base(), {
  type: "SET_STATE",
  state: snapshot(1, { orch, "worker-live": liveWorker, "worker-gone": goneWorker }),
});

test("both workers render before the runtime close", () => {
  const ids = subtreeIds(withBoth.panes);
  assert(ids.includes("worker-live"), "live worker present");
  assert(ids.includes("worker-gone"), "gone worker present pre-close");
});

// Runtime close: a fresh higher-generation snapshot prunes worker-gone; the
// reducer must drop it from state.panes.
const afterClose = reducer(withBoth, {
  type: "SET_STATE",
  state: snapshot(2, { orch, "worker-live": liveWorker }),
});

test("closed worker leaves state.panes on the next snapshot", () => {
  assert(!afterClose.panes["worker-gone"], "worker-gone pruned from state.panes");
  assert(afterClose.panes["worker-live"], "live worker retained");
});

test("closed worker is not rendered as a live child after the close", () => {
  const ids = subtreeIds(afterClose.panes);
  assert(ids.includes("worker-live"), "live worker still rendered");
  assert(!ids.includes("worker-gone"), "closed worker must NOT render as a live child");
});

// Poisoned fixture: a stale status entry for worker-gone (missed Dead event) must
// not resurrect a row — rows derive from state.panes membership, not the status map.
test("stale status entry for a gone pane does not resurrect a row", () => {
  assert(statuses["worker-gone"] === "Working", "fixture keeps the stale status entry");
  const nodes = buildHierarchySubtree("orch", afterClose.panes, statuses);
  const ids = nodes.flatMap((n) => [n.pane.id, ...n.children.map((c) => c.pane.id)]);
  assert(!ids.includes("worker-gone"), "stale status fixture must not add a ghost row");
});

if (failures > 0) {
  console.error(`\n${failures} test(s) failed`);
  process.exit(1);
}
console.log("\n✓ hierarchy runtime-close: pruned worker never renders as a ghost row");
