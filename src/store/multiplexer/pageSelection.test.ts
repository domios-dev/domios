import { describe, expect, it } from "vitest";
import { isWorkspaceTabActive } from "../../components/Sidebar/sidebarSelection";
import type { MultiplexerState, Tab } from "../../lib/types";
import { buildInitialState } from "./initialState";
import { reducer } from "./reducer";
import type { ExtendedState } from "./tree";
import { acceptsPageFocus, initialPageSelection, initialRouteReady, reconcilePageSelection } from "./pageSelection";
import { mountedChatWindow } from "../../components/Terminal/chatMountedWindow";

const previous = { active_workspace_id: "qa", focused_tab_id: "chat", activeTabIds: { qa: "chat", other: "worker" } };
const incoming = { active_workspace_id: "other", focused_tab_id: "audit", activeTabIds: { qa: "audit", other: "shell" } };
const workspaces = [{ id: "qa", tabs: ["chat", "audit"] }, { id: "other", tabs: ["worker", "shell"] }];

describe("initial page route", () => {
  const snapshot = { active_workspace_id: "qa", focused_tab_id: "shell",
    workspaces: [{ ...workspaces[0], active_tab_id: "chat" }, { ...workspaces[1], active_tab_id: "shell" }],
    tabs: { chat: {}, audit: {}, worker: {}, shell: {} } };
  it("restores workspace and chat together before reconciliation", () => {
    expect(initialPageSelection(snapshot)).toEqual({ active_workspace_id: "qa", focused_tab_id: "chat",
      activeTabIds: { qa: "chat", other: "shell" } });
  });
  it("prefers the page's saved workspace and tab over another client's daemon focus", () => {
    const route = initialPageSelection(snapshot, { workspaceId: "other", tabId: "worker" });
    expect(route.active_workspace_id).toBe("other");
    expect(route.focused_tab_id).toBe("worker");
    expect(route.activeTabIds.other).toBe("worker");
  });
  it("uses daemon focus when the saved workspace or its exact tab is missing", () => {
    for (const saved of [{ workspaceId: "removed", tabId: "worker" },
      { workspaceId: "other", tabId: "removed" }, { workspaceId: "other", tabId: "chat" }]) {
      expect(initialPageSelection(snapshot, saved)).toEqual(initialPageSelection(snapshot));
    }
  });
  it("uses daemon focus for a page without a saved route", () => {
    expect(initialPageSelection(snapshot, null).active_workspace_id).toBe("qa");
    expect(initialPageSelection(snapshot, null).focused_tab_id).toBe("chat");
  });
  it("falls back to surviving tabs and workspaces", () => {
    expect(initialPageSelection({ ...snapshot, active_workspace_id: "removed", tabs: { audit: {} } }))
      .toEqual({ active_workspace_id: "qa", focused_tab_id: "audit", activeTabIds: { qa: "audit" } });
  });
  it("allows an empty workspace only after a snapshot exists", () => {
    expect(initialRouteReady(0)).toBe(false);
    expect(initialRouteReady(0.5)).toBe(true);
    expect(initialRouteReady(1)).toBe(true);
    expect(initialPageSelection({ active_workspace_id: null, workspaces: [], tabs: {} }))
      .toEqual({ active_workspace_id: null, focused_tab_id: null, activeTabIds: {} });
  });
});

describe("page selection during daemon updates", () => {
  it("keeps a confirmed live chat active when another client selects Audit", () => {
    const confirmed = reconcilePageSelection(previous, previous, workspaces, true);
    const selected = reconcilePageSelection(confirmed, incoming, workspaces, true);
    expect(selected).toEqual(previous);
    const active = selected.activeTabIds.qa === "chat" && selected.active_workspace_id === "qa";
    const range = mountedChatWindow({ heights: Array(171).fill(120), scrollTop: 26279,
      viewportHeight: 717, pinned: true, active });
    expect(range.end).toBe(171);
    expect(range.start).toBeLessThan(170);
    expect(acceptsPageFocus(selected, "audit", true)).toBe(false);
    expect(acceptsPageFocus(selected, "chat", true)).toBe(true);
  });
  it("accepts an explicit local switch while other pages retain their selection", () => {
    const local = { ...previous, focused_tab_id: "audit", activeTabIds: { ...previous.activeTabIds, qa: "audit" } };
    expect(reconcilePageSelection(local, previous, workspaces, true)).toEqual(local);
    expect(reconcilePageSelection(previous, local, workspaces, true)).toEqual(previous);
  });
  it("uses authoritative fallback when the selected tab or workspace is removed", () => {
    const withoutChat = [{ id: "qa", tabs: ["audit"] }, workspaces[1]];
    expect(reconcilePageSelection(previous, incoming, withoutChat, true))
      .toEqual({ ...previous, focused_tab_id: "audit", activeTabIds: { qa: "audit", other: "worker" } });
    expect(reconcilePageSelection(previous, incoming, [workspaces[1]], true))
      .toEqual({ active_workspace_id: "other", focused_tab_id: "worker", activeTabIds: { qa: "audit", other: "worker" } });
  });
  it("hydrates an empty page and preserves native daemon selection semantics", () => {
    const empty = { active_workspace_id: null, focused_tab_id: null, activeTabIds: {} };
    expect(reconcilePageSelection(empty, incoming, workspaces, true))
      .toEqual({ ...incoming, focused_tab_id: "shell" });
    expect(reconcilePageSelection(previous, incoming, workspaces, false)).toBe(incoming);
    expect(acceptsPageFocus(previous, "audit", false)).toBe(true);
  });
});

function tab(id: string): Tab {
  return { id, title: id, pty_id: null, cwd: null, tab_type: "Terminal" };
}

function surface(id: string, parent_tab_id: string | null): Tab {
  return {
    ...tab(id),
    parent_tab_id,
    backing: { kind: "surface", plugin_id: "browser", surface_id: "browser", instance_id: id, owner_label: true },
  };
}

function fixture(owner: Tab | null): MultiplexerState {
  const tabs = { ...(owner ? { [owner.id]: owner } : {}), a: surface("a", owner?.id ?? null), b: surface("b", null) };
  return {
    generation: 1,
    active_workspace_id: "w",
    focused_tab_id: "a",
    workspaces: [{ id: "w", name: "W", tabs: Object.keys(tabs), active_tab_id: "a", cwd: null }],
    tabs,
  };
}

function start(snapshot: MultiplexerState): ExtendedState {
  return reducer(buildInitialState(), { type: "SET_STATE", state: snapshot, pageOwnedSelection: true });
}

function close(state: ExtendedState, tabId: string): ExtendedState {
  return reducer(state, { type: "CLOSE_TAB", workspaceId: "w", tabId, removedTabIds: [tabId] });
}

function acknowledge(state: ExtendedState, activeTabId: string | null): ExtendedState {
  const { a: _closed, ...tabs } = state.tabs;
  return reducer(state, {
    type: "SET_STATE",
    pageOwnedSelection: true,
    state: {
      generation: 2,
      active_workspace_id: "w",
      focused_tab_id: activeTabId,
      tabs,
      workspaces: [{ ...state.workspaces[0], active_tab_id: activeTabId, tabs: Object.keys(tabs) }],
    },
  });
}

function expectSelection(state: ExtendedState, tabId: string | undefined) {
  const active = state.activeTabIds.w;
  const sidebar = state.workspaces[0].tabs.filter((id) => isWorkspaceTabActive(true, false, id, active));
  expect(active).toBe(tabId);
  expect(sidebar).toEqual(tabId ? [tabId] : []);
  expect(state.tabs.b?.backing?.kind).toBe("surface");
}

describe("browser selection after close", () => {
  it("selects the live owner at optimistic close and keeps it at acknowledgement", () => {
    const closed = close(start(fixture(tab("owner"))), "a");
    expectSelection(closed, "owner");
    expectSelection(acknowledge(closed, "owner"), "owner");
  });

  it("uses the local survivor when another page was focused on a different browser", () => {
    expectSelection(acknowledge(start(fixture(tab("owner"))), "b"), "owner");
  });

  it("keeps the selection empty when only another surface remains", () => {
    const closed = close(start(fixture(null)), "a");
    expectSelection(closed, undefined);
    const acknowledged = acknowledge(closed, null);
    expectSelection(acknowledged, undefined);
    expect(initialPageSelection(acknowledged).activeTabIds.w).toBeUndefined();
  });

  it("keeps an explicitly selected browser when an inactive surface closes", () => {
    const state = start(fixture(tab("owner")));
    const selected = reducer(state, { type: "SET_ACTIVE_TAB", workspaceId: "w", tabId: "b", source: "user" });
    expectSelection(close(selected, "a"), "b");
  });
});
