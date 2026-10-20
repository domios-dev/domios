import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { sameTabItemRow, type SidebarTabItemRowProps } from "./SidebarPlanPeerPair";
import { selectSidebarState, sidebarStateEqual } from "./sidebarState";
import { buildInitialState } from "../../store/multiplexer/initialState";

const tab = (id: string, extra: Partial<Tab> = {}): Tab => ({
  id, title: id, pty_id: null, tab_type: "Terminal", cwd: null, agent_type: "claude",
  agent_state: { state: "working", status_line: "Thinking" }, ...extra,
});
const noop = () => {};
const row = (extra: Partial<SidebarTabItemRowProps> = {}): SidebarTabItemRowProps => ({
  tab: tab("a"), index: 0, activity: null, isClosing: false, isAppearing: false, workspaceId: "w1",
  activeTabId: "x", isActiveVisual: true, workspaceCanvasActive: false, tabStatuses: { a: "Working", b: "Idle" },
  orchestrators: [], touchMode: false, onActivateTab: noop, onCloseTab: noop, onRenameTab: noop, ...extra,
});

describe("sidebar row memo", () => {
  it("skips a row when only other tabs' status or selection changed", () => {
    expect(sameTabItemRow(row(), row({ tabStatuses: { a: "Working", b: "Working" } }))).toBe(true);
    expect(sameTabItemRow(row(), row({ activeTabId: "y" }))).toBe(true);
    expect(sameTabItemRow(row(), row({ tab: tab("a", { last_active_at_ms: 99 }) }))).toBe(true);
  });

  it("renders a row when its own state changes", () => {
    expect(sameTabItemRow(row(), row({ activeTabId: "a" }))).toBe(false);
    expect(sameTabItemRow(row(), row({ tabStatuses: { a: "Idle", b: "Idle" } }))).toBe(false);
    expect(sameTabItemRow(row(), row({ tab: tab("a", { agent_state: { state: "working", status_line: "Writing" } }) }))).toBe(false);
    expect(sameTabItemRow(row(), row({ onActivateTab: () => {} }))).toBe(false);
  });

  it("keeps the sidebar still when only activity timestamps move", () => {
    const state = { ...buildInitialState(), tabs: { a: tab("a", { last_active_at_ms: 1 }) } };
    const moved = { ...state, tabs: { a: tab("a", { last_active_at_ms: 2 }) } };
    expect(sidebarStateEqual(selectSidebarState(state), selectSidebarState(moved))).toBe(true);
    const renamed = { ...state, tabs: { a: tab("a", { title: "Renamed" }) } };
    expect(sidebarStateEqual(selectSidebarState(state), selectSidebarState(renamed))).toBe(false);
  });
});
