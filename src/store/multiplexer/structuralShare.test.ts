import { describe, expect, it } from "vitest";
import type { MultiplexerState, Tab } from "../../lib/types";
import { sameJson, shareRecord } from "../../lib/sameJson";
import { buildInitialState } from "./initialState";
import { reducer } from "./reducer";
import { snapWithTabs } from "./reducer.test.fixtures";
import { appShellStateEqual } from "./selectors";
import { shareList } from "./structuralShare";
import type { ExtendedState } from "./tree";

function accept(state: ExtendedState, snapshot: MultiplexerState): ExtendedState {
  return reducer(state, { type: "SET_STATE", state: snapshot });
}

function withStatus(snapshot: MultiplexerState, tabId: string, status: string): MultiplexerState {
  return { ...snapshot, tabs: { ...snapshot.tabs, [tabId]: { ...snapshot.tabs[tabId], status_line: status } as unknown as Tab } };
}

describe("snapshot structural sharing", () => {
  it("keeps every identity when a snapshot changes only the generation", () => {
    const first = accept(buildInitialState(), snapWithTabs(1, [["w1", "a"], ["w1", "b"]]));
    const second = accept(first, snapWithTabs(2, [["w1", "a"], ["w1", "b"]]));
    expect(second.generation).toBe(2);
    expect(second.tabs).toBe(first.tabs);
    expect(second.workspaces).toBe(first.workspaces);
    expect(second.tabStatuses).toBe(first.tabStatuses);
    expect(second.activeTabIds).toBe(first.activeTabIds);
    expect(appShellStateEqual(first, second)).toBe(true);
  });

  it("replaces only the tab whose content changed", () => {
    const snapshot = snapWithTabs(1, [["w1", "a"], ["w1", "b"]]);
    const first = accept(buildInitialState(), snapshot);
    const second = accept(first, withStatus({ ...snapshot, generation: 2 }, "b", "Thinking"));
    expect(second.tabs).not.toBe(first.tabs);
    expect(second.tabs.a).toBe(first.tabs.a);
    expect(second.tabs.b).not.toBe(first.tabs.b);
    expect(second.workspaces).toBe(first.workspaces);
    expect(appShellStateEqual(first, second)).toBe(false);
  });

  it("keeps remote tab projections that did not change", () => {
    const first = accept(buildInitialState(), snapWithTabs(1, [["w1", "a"]]));
    const remote = { r: { id: "r", title: "Remote", remote_peer_id: "peer" } as unknown as Tab };
    const attached = reducer(first, { type: "SET_REMOTE_TABS", tabs: remote });
    const again = reducer(attached, { type: "SET_REMOTE_TABS", tabs: { r: { ...remote.r } } });
    expect(again).toBe(attached);
  });

  it("treats route readiness as the only generation change the app shell sees", () => {
    const first = accept(buildInitialState(), snapWithTabs(1, [["w1", "a"]]));
    expect(appShellStateEqual({ ...first, generation: 0.5 }, first)).toBe(false);
    expect(appShellStateEqual({ ...first, generation: 7 }, first)).toBe(true);
  });
});

describe("structural equality", () => {
  it("compares plain records and arrays by content", () => {
    expect(sameJson({ a: [1, { b: null }] }, { a: [1, { b: null }] })).toBe(true);
    expect(sameJson({ a: 1 }, { a: 1, b: undefined })).toBe(false);
    expect(sameJson([1, 2], { 0: 1, 1: 2 })).toBe(false);
  });

  it("never equates distinct maps, sets or functions", () => {
    expect(sameJson(new Map([["a", 1]]), new Map())).toBe(false);
    expect(sameJson(new Set([1]), new Set([1]))).toBe(false);
    expect(sameJson(() => 1, () => 1)).toBe(false);
  });

  it("returns the previous record or list when nothing changed", () => {
    const previous = { a: { x: 1 }, b: { x: 2 } };
    expect(shareRecord(previous, { a: { x: 1 }, b: { x: 2 } })).toBe(previous);
    const changed = shareRecord(previous, { a: { x: 1 }, b: { x: 3 } });
    expect(changed.a).toBe(previous.a);
    expect(changed.b).toEqual({ x: 3 });
    const list = [{ id: "a", n: 1 }, { id: "b", n: 2 }];
    expect(shareList(list, [{ id: "a", n: 1 }, { id: "b", n: 2 }], (item) => item.id)).toBe(list);
    const reordered = shareList(list, [{ id: "b", n: 2 }, { id: "a", n: 1 }], (item) => item.id);
    expect(reordered).not.toBe(list);
    expect(reordered[0]).toBe(list[1]);
  });
});
