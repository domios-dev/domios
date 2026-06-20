// Run: npx tsx src/store/multiplexer/reducer.test.ts
//
// SET_STATE monotonic generation guard (T3 Fix B): a snapshot whose generation
// is <= the applied one is ignored, so a stale `mux_get_state` that resolves
// after a newer snapshot can no longer clobber the just-created tab.

import { reducer } from "./reducer";
import type { ExtendedState } from "./tree";
import type { MultiplexerState, Workspace, Pane } from "../../lib/types";

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

function base(): ExtendedState {
  return {
    workspaces: [],
    panes: {},
    active_workspace_id: null,
    focused_pane_id: null,
    notifications: [],
    paneStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
  };
}

function snap(generation: number, wsId: string): MultiplexerState {
  const ws = {
    id: wsId,
    name: "W",
    tabs: [],
    active_tab_id: null,
    cwd: null,
  } as unknown as Workspace;
  return {
    workspaces: [ws],
    panes: {},
    active_workspace_id: wsId,
    focused_pane_id: null,
    generation,
  };
}

test("a newer snapshot is applied", () => {
  const next = reducer(base(), { type: "SET_STATE", state: snap(5, "w-new") });
  assert(next.generation === 5, "generation advances to 5");
  assert(next.active_workspace_id === "w-new", "workspace applied");
});

test("a stale (older generation) snapshot is ignored", () => {
  const applied = reducer(base(), { type: "SET_STATE", state: snap(5, "w-new") });
  const after = reducer(applied, { type: "SET_STATE", state: snap(3, "w-stale") });
  assert(after === applied, "stale snapshot returns the same state reference");
  assert(after.active_workspace_id === "w-new", "stale snapshot does not clobber");
  assert(after.generation === 5, "applied generation unchanged");
});

test("an equal-generation snapshot is ignored", () => {
  const applied = reducer(base(), { type: "SET_STATE", state: snap(5, "w-new") });
  const after = reducer(applied, { type: "SET_STATE", state: snap(5, "w-dup") });
  assert(after === applied, "equal generation returns the same state");
  assert(after.active_workspace_id === "w-new", "equal generation does not clobber");
});

test("snapshots without a generation still apply (back-compat)", () => {
  const noGen = { ...snap(0, "w-x") } as MultiplexerState;
  delete (noGen as { generation?: number }).generation;
  const next = reducer(base(), { type: "SET_STATE", state: noGen });
  assert(next.active_workspace_id === "w-x", "ungenerationed snapshot applied");
});

// SET_STATE focused_pane_id: under the payload-based mux-state-changed flow,
// snapshots now carry focused_pane_id. The reducer must adopt it when it names
// a pane present in the merged set (initial hydrate / restore), but must NOT
// clobber a valid focus with a stale value naming a missing pane.
function pane(id: string): Pane {
  return { id, pty_id: null, title: id } as unknown as Pane;
}

function snapWithFocus(
  generation: number,
  paneIds: string[],
  focused: string | null,
): MultiplexerState {
  const panes: Record<string, Pane> = {};
  for (const id of paneIds) panes[id] = pane(id);
  return {
    workspaces: [],
    panes,
    active_workspace_id: null,
    focused_pane_id: focused,
    generation,
  };
}

test("initial hydrate adopts focused_pane_id naming an existing pane", () => {
  const next = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  assert(next.focused_pane_id === "p1", "focus adopted from snapshot");
});

test("snapshot naming a missing pane preserves prior focus", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: snapWithFocus(2, ["p1"], "ghost"),
  });
  assert(
    after.focused_pane_id === "p1",
    "stale focus naming a missing pane does not clobber valid focus",
  );
});

test("snapshot with null focus preserves prior focus", () => {
  const hydrated = reducer(base(), {
    type: "SET_STATE",
    state: snapWithFocus(1, ["p1"], "p1"),
  });
  const after = reducer(hydrated, {
    type: "SET_STATE",
    state: snapWithFocus(2, ["p1"], null),
  });
  assert(after.focused_pane_id === "p1", "null focus does not clobber valid focus");
});

console.log(failures === 0 ? "✓ reducer tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
