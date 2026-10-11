import { describe, expect, it } from "vitest";
import { acceptsPageFocus, reconcilePageSelection } from "./pageSelection";
import { mountedChatWindow } from "../../components/Terminal/chatMountedWindow";

const previous = { active_workspace_id: "qa", focused_tab_id: "chat", activeTabIds: { qa: "chat", other: "worker" } };
const incoming = { active_workspace_id: "other", focused_tab_id: "audit", activeTabIds: { qa: "audit", other: "shell" } };
const workspaces = [{ id: "qa", tabs: ["chat", "audit"] }, { id: "other", tabs: ["worker", "shell"] }];

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
