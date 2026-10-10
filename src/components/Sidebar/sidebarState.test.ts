import assert from "node:assert/strict";
import test from "node:test";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { selectSidebarState, sidebarStateEqual } from "./sidebarState";

function fixture(): ExtendedState {
  return {
    tabs: { one: { id: "one", title: "First", agent_state: { state: "busy", elapsed_ms: 1, status_line: "Working" } } },
    workspaces: [{ id: "ws", name: "Workspace", tabs: ["one"], active_tab_id: "one" }],
    teams: [], active_workspace_id: "ws", activeTabIds: { ws: "one" },
    notifications: [], tabStatuses: {}, recently_closed: [], closed_structures: [],
    generation: 1,
  } as unknown as ExtendedState;
}

test("sidebar ignores equal snapshots, generation and elapsed agent time", () => {
  const before = fixture();
  const after = structuredClone(before);
  after.generation = 2;
  after.tabs.one.agent_state!.elapsed_ms = 99;
  assert.ok(sidebarStateEqual(selectSidebarState(before), selectSidebarState(after)));
});

test("sidebar preserves visible tab, workspace, attention and close updates", () => {
  const mutations: Array<(state: ExtendedState) => void> = [
    (state) => { state.tabs.one.title = "Renamed"; },
    (state) => { state.tabs.one.agent_session_model = "new-model"; },
    (state) => { state.tabs.one.agent_state!.status_line = "Ready"; },
    (state) => { delete state.tabs.one; },
    (state) => { state.workspaces[0].tabs.push("two"); },
    (state) => { state.workspaces[0].name = "Renamed workspace"; },
    (state) => { state.activeTabIds.ws = "two"; },
    (state) => { state.notifications = [{ id: "attention" }] as ExtendedState["notifications"]; },
    (state) => { state.recently_closed = [{ tab_id: "closed" }] as unknown as ExtendedState["recently_closed"]; },
  ];
  for (const mutate of mutations) {
    const before = fixture();
    const after = structuredClone(before);
    mutate(after);
    assert.equal(sidebarStateEqual(selectSidebarState(before), selectSidebarState(after)), false);
  }
});

