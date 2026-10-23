import { describe, expect, it } from "vitest";
import { reducer } from "./reducer";
import { seededSelection } from "./pageSelection";
import { snapWithTabs } from "./reducer.test.fixtures";
import type { ExtendedState } from "./tree";
import type { MultiplexerState, Workspace } from "../../lib/types";

function twoWorkspaces(generation: number): MultiplexerState {
  const first = snapWithTabs(generation, [["w1", "a1"], ["w1", "a2"]]);
  const second = snapWithTabs(generation, [["w2", "b1"]]);
  const workspace2 = { ...second.workspaces[0], id: "w2", tabs: ["b1"], active_tab_id: "b1" } as Workspace;
  return { ...first, workspaces: [first.workspaces[0], workspace2], tabs: { ...first.tabs, ...second.tabs }, active_workspace_id: "w1" };
}

function emptyState(): ExtendedState {
  return {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
  } as unknown as ExtendedState;
}

const restored = { active_workspace_id: "w2", focused_tab_id: "b1", activeTabIds: { w1: "a1", w2: "b1" } };

function land(state: ExtendedState): ExtendedState {
  return reducer(state, { type: "SET_STATE", state: twoWorkspaces(5), pageOwnedSelection: true, initialSelection: restored });
}

describe("restore versus user navigation ordering", () => {
  it("a restore seed applies only while the user has not navigated", () => {
    expect(seededSelection(false, restored)).toBe(restored);
    expect(seededSelection(true, restored)).toBeUndefined();
    expect(seededSelection(false, undefined)).toBeUndefined();
  });

  it("restores the saved selection when nothing was navigated, and counts the seed", () => {
    const after = land(emptyState());
    expect(after.active_workspace_id).toBe("w2");
    expect(after.activeTabIds.w2).toBe("b1");
    expect(after.focused_tab_id).toBe("b1");
    expect(after.selectionSeeds).toBe(1);
  });

  it("a user tab selection made before the restore lands wins over it", () => {
    let state = emptyState();
    state = reducer(state, { type: "SET_ACTIVE_WORKSPACE", id: "w1" });
    state = reducer(state, { type: "SET_ACTIVE_TAB", workspaceId: "w1", tabId: "a2" });
    const after = land(state);
    expect(after.active_workspace_id).toBe("w1");
    expect(after.activeTabIds.w1).toBe("a2");
    expect(after.focused_tab_id).toBe("a2");
    expect(after.selectionSeeds ?? 0).toBe(0);
  });

  it("a workspace selection alone also blocks a later restore from stealing the view", () => {
    const after = land(reducer(emptyState(), { type: "SET_ACTIVE_WORKSPACE", id: "w1" }));
    expect(after.active_workspace_id).toBe("w1");
  });

  it("a user selection made after the restore landed still wins", () => {
    const seeded = land(emptyState());
    const after = reducer(seeded, { type: "SET_ACTIVE_WORKSPACE", id: "w1" });
    expect(after.active_workspace_id).toBe("w1");
    expect(land(after).active_workspace_id).toBe("w1");
  });
});
