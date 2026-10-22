import assert from "node:assert/strict";
import { test } from "node:test";
import type { Tab } from "../../lib/types";
import { reducer } from "./reducer";
import { isMuxStatusDelta, statusDeltaAction, type MuxStatusDelta } from "./statusDelta";
import type { ExtendedState } from "./tree";

function fixture(): ExtendedState {
  return {
    workspaces: [{ id: "w", name: "W", tabs: ["a", "b"], active_tab_id: "b", cwd: null }],
    tabs: { a: { id: "a", title: "A" } as Tab, b: { id: "b", title: "B" } as Tab },
    active_workspace_id: "w", focused_tab_id: "b", activeTabIds: { w: "b" },
    teams: [], notifications: [], toasts: [], tabStatuses: { a: "Working", b: "Waiting" },
    pendingCloses: [], generation: 10,
  };
}

function delta(base = 10, generation = 15): MuxStatusDelta {
  return { base_generation: base, generation, status_deltas: { a: { status: "Idle", agent_state: { state: "idle" } } } };
}

test("a coalesced status delta updates only the changed tab and preserves page selection", () => {
  const state = fixture();
  const next = reducer(state, { type: "APPLY_STATUS_DELTA", delta: delta() });
  assert.equal(next.generation, 15);
  assert.equal(next.tabStatuses.a, "Idle");
  assert.deepEqual(next.tabs.a.agent_state, { state: "idle" });
  assert.equal(next.tabs.b, state.tabs.b);
  assert.equal(next.workspaces, state.workspaces);
  assert.equal(next.teams, state.teams);
  assert.equal(next.activeTabIds, state.activeTabIds);
  assert.equal(next.focused_tab_id, "b");
  assert.equal(next.notifications, state.notifications);
});

test("removal clears a stale status and agent state without reviving closed tabs", () => {
  const state = fixture();
  state.pendingCloses = ["b"];
  const next = reducer(state, { type: "APPLY_STATUS_DELTA", delta: {
    ...delta(), status_deltas: {
      a: { status: null, agent_state: null },
      b: { status: "Dead", agent_state: null },
      missing: { status: "Working", agent_state: null },
    },
  } });
  assert.deepEqual(next.tabStatuses, { b: "Waiting" });
  assert.equal(next.tabs.a.agent_state, null);
  assert.equal(next.tabs.b, state.tabs.b);
  assert.equal(next.tabs.missing, undefined);
  assert.equal(next.pendingCloses, state.pendingCloses);
});

test("a declined delta records a resync without changing displayed status or tabs", () => {
  const state = fixture();
  for (const update of [delta(5, 9), delta(5, 10), delta(9, 15), delta(11, 15)]) {
    const declined = reducer(state, { type: "APPLY_STATUS_DELTA", delta: update });
    assert.equal(declined.muxResyncRequired, true);
    assert.equal(declined.tabs, state.tabs);
    assert.equal(declined.tabStatuses, state.tabStatuses);
    assert.equal(declined.generation, 10);
  }
  assert.equal(statusDeltaAction(10, delta(9, 15)), "refresh");
  assert.equal(statusDeltaAction(undefined, delta()), "refresh");
  assert.equal(statusDeltaAction(-1, delta()), "refresh");
  assert.equal(statusDeltaAction(10, delta(5, 10)), "ignore");
  assert.equal(statusDeltaAction(10, delta()), "apply");
  assert.equal(isMuxStatusDelta(delta()), true);
  assert.equal(isMuxStatusDelta({ generation: 15 }), false);
});

test("a forced published snapshot recovers a reducer outside the stream generation", () => {
  const state = fixture();
  const ahead = reducer(state, { type: "SET_STATE", state: { ...state, generation: 100 } });
  const declined = reducer(ahead, { type: "APPLY_STATUS_DELTA", delta: delta(10, 11) });
  assert.equal(declined.muxResyncRequired, true);
  const restored = reducer(declined, { type: "SET_STATE", force: true, state: { ...state, generation: 11 } });
  assert.equal(restored.generation, 11);
  assert.equal(restored.muxResyncRequired, false);
  const next = reducer(restored, { type: "APPLY_STATUS_DELTA", delta: delta(11, 12) });
  assert.equal(next.tabStatuses.a, "Idle");
  assert.equal(next.generation, 12);
});

test("an unchanged published snapshot can restore a rejected optimistic close", () => {
  const state = fixture();
  const closed = reducer(state, { type: "CLOSE_TAB", workspaceId: "w", tabId: "b", removedTabIds: ["b"] });
  const rejected = reducer(closed, { type: "REMOVE_PENDING_CLOSE", tabIds: ["b"] });
  assert.equal(reducer(rejected, { type: "SET_STATE", state }), rejected);
  const restored = reducer(rejected, { type: "SET_STATE", state, force: true });
  assert.equal(restored.generation, 10);
  assert.ok(restored.tabs.b);
  assert.deepEqual(restored.workspaces[0].tabs, ["a", "b"]);
  assert.deepEqual(restored.pendingCloses, []);
});

test("equal decoded agent state retains the tab identity while status changes", () => {
  const state = fixture();
  state.tabs.a.agent_state = { state: "idle", status_line: "opaque-phase" };
  const update = delta();
  update.status_deltas.a.agent_state = { state: "idle", status_line: "opaque-phase" };
  const next = reducer(state, { type: "APPLY_STATUS_DELTA", delta: update });
  assert.equal(next.tabStatuses.a, "Idle");
  assert.equal(next.tabs, state.tabs);
  assert.equal(next.tabs.a, state.tabs.a);
  const again = reducer(next, { type: "APPLY_STATUS_DELTA", delta: { ...update, base_generation: 15, generation: 18 } });
  assert.equal(again.tabs, next.tabs);
  assert.equal(again.tabStatuses, next.tabStatuses);
});

test("126 status and 16 structural events across five 221-tab clients cut refetches from 710 to 80", () => {
  const clients = Array.from({ length: 5 }, () => {
    const state = fixture();
    for (let i = 2; i < 221; i++) state.tabs[`tab-${i}`] = { id: `tab-${i}`, title: `Tab ${i}` } as Tab;
    return state;
  });
  let reads = 0;
  let generation = 10;
  for (let i = 0; i < 142; i++) {
    const base = generation;
    generation += 3;
    for (let c = 0; c < clients.length; c++) {
      if (i < 126) {
        const update = delta(base, generation);
        assert.equal(statusDeltaAction(clients[c].generation, update), "apply");
        clients[c] = reducer(clients[c], { type: "APPLY_STATUS_DELTA", delta: update });
      } else {
        reads++;
        clients[c] = reducer(clients[c], { type: "SET_STATE", state: {
          ...clients[c], generation, tab_statuses: clients[c].tabStatuses,
        } });
      }
    }
  }
  assert.equal(reads, 80);
  assert.equal(142 * clients.length, 710);
  assert.ok(clients.every((client) => client.generation === generation && client.tabStatuses.a === "Idle"));
});
