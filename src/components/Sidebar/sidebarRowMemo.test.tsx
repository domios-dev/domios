import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { useSortable } from "@dnd-kit/sortable";
import type { Tab } from "../../lib/types";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { sameTabItemRow, type SidebarTabItemRowProps } from "./SidebarPlanPeerPair";
import { selectSidebarState, sidebarStateEqual } from "./sidebarState";
import { SidebarDnd, SortableContext } from "./sortable";
import { useLiveRowTab } from "./useLiveRowTab";
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

  it("leaves a working tab's streamed line to its row but re-renders on a state change", () => {
    const at = (agent_state: Tab["agent_state"]) => selectSidebarState({ ...buildInitialState(), tabs: { a: tab("a", { agent_state }) } });
    const working = at({ state: "working", status_line: "Thinking 1s", phase: "thinking" });
    expect(sidebarStateEqual(working, at({ state: "working", status_line: "Writing 2s", phase: "streaming", elapsed_ms: 2000 }))).toBe(true);
    expect(sidebarStateEqual(working, at({ state: "idle", status_line: "Thinking 1s", phase: "thinking" }))).toBe(false);
    const question = at({ state: "question", status_line: "Allow edit?" });
    expect(sidebarStateEqual(question, at({ state: "question", status_line: "Allow write?" }))).toBe(false);
  });

  it("gives a row its tab's live agent state from the store", async () => {
    const stale = tab("a", { agent_state: { state: "working", status_line: "Thinking" } });
    let current = { ...buildInitialState(), tabs: { a: stale } } as ExtendedState;
    const listeners = new Set<() => void>();
    const store = { getState: () => current, subscribe: (listener: () => void) => { listeners.add(listener); return () => listeners.delete(listener); }, notify: () => {} };
    const seen: (string | null | undefined)[] = [];
    function Probe() { seen.push(useLiveRowTab(stale).agent_state?.status_line); return null; }
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const root = createRoot(document.createElement("div"));
    await act(async () => root.render(<MuxStateStoreContext.Provider value={store}><Probe /></MuxStateStoreContext.Provider>));
    current = { ...current, tabs: { a: { ...stale, agent_state: { state: "working", status_line: "Writing" } } } };
    await act(async () => { for (const listener of listeners) listener(); });
    expect(seen[seen.length - 1]).toBe("Writing");
    await act(async () => root.unmount());
  });

  it("keeps sortable rows still when a parent passes equal ids in a new array", async () => {
    let renders = 0;
    function Row() { renders++; useSortable({ id: "tab:a" }); return null; }
    const handlers = { reorderWorkspaces: noop, reorderTabs: noop, reorderDisplay: noop };
    const row = <Row />;
    const tree = () => <SidebarDnd handlers={handlers}><SortableContext items={["tab:a"]}>{row}</SortableContext></SidebarDnd>;
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const root = createRoot(document.createElement("div"));
    await act(async () => root.render(tree()));
    const before = renders;
    await act(async () => root.render(tree()));
    expect(renders).toBe(before);
    await act(async () => root.unmount());
  });
});
