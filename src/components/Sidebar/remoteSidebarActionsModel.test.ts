import { describe, expect, it } from "vitest";
import {
  remoteObservedTabAllowedMenuIds,
  remotePeerActionEligibility,
  remotePeerStatusLabel,
  remoteTabCloseAction,
  remoteTeamMenuActions,
  remoteWorkspaceCloseAction,
  remoteWorkspaceCreateAction,
  remoteWorkspaceMenuActions,
  workspaceNameFromPath,
} from "./remoteSidebarActionsModel";
import { buildTabMenu, unavailableTabMenu } from "../Terminal/tabMenuModel";
import type { Tab } from "../../lib/types";
import type { GeneralAvailability } from "../../store/multiplexer/pairedWorkspaceProjection";

const UNAVAILABLE_STATES: ReadonlyArray<[GeneralAvailability, string]> = [
  ["offline", "offline"],
  ["signed-out", "signed out"],
  ["update-required", "update required"],
  ["reconnecting", "reconnecting"],
  ["unreachable", "unreachable"],
];

const remoteTab = (overrides: Partial<Tab> = {}): Tab => ({
  id: "mesh:peer-b/tab-1",
  title: "Agent",
  pty_id: null,
  tab_type: "Terminal",
  cwd: "/repo",
  agent_type: "codex",
  remote_peer_id: "peer-b",
  remote_peer_name: "Desk B",
  remote_tab_id: "tab-1",
  ...overrides,
});

describe("remote sidebar actions decision model", () => {
  it("enables remote actions only when the peer is online and reports exact status labels otherwise", () => {
    expect(remotePeerStatusLabel("online")).toBe("online");
    expect(remotePeerActionEligibility("online")).toEqual({ allowed: true, reason: null });
    expect(remotePeerActionEligibility(undefined)).toEqual({ allowed: false, reason: "offline" });
    for (const [availability, label] of UNAVAILABLE_STATES) {
      expect(remotePeerStatusLabel(availability)).toBe(label);
      expect(remotePeerActionEligibility(availability)).toEqual({ allowed: false, reason: label });
    }
  });

  it("enables workspace create for local and online remote peers, and disables it with status label otherwise", () => {
    expect(remoteWorkspaceCreateAction(null)).toEqual({ visible: true, disabled: false, reason: null });
    expect(remoteWorkspaceCreateAction("peer-b", "online")).toEqual({ visible: true, disabled: false, reason: null });
    for (const [availability, label] of UNAVAILABLE_STATES) {
      expect(remoteWorkspaceCreateAction("peer-b", availability)).toEqual({
        visible: true,
        disabled: true,
        reason: label,
      });
    }
  });

  it("never offers Close on protected remote workspaces and disables Close with status label when peer is unavailable", () => {
    expect(remoteWorkspaceCloseAction(true, "online")).toEqual({ visible: false, disabled: true, reason: null });
    expect(remoteWorkspaceCloseAction(true, "offline")).toEqual({ visible: false, disabled: true, reason: null });
    expect(remoteWorkspaceCloseAction(false, "online")).toEqual({ visible: true, disabled: false, reason: null });
    for (const [availability, label] of UNAVAILABLE_STATES) {
      expect(remoteWorkspaceCloseAction(false, availability)).toEqual({
        visible: true,
        disabled: true,
        reason: label,
      });
    }
  });

  it("never exposes local Save as template for remote Teams and scopes Rename and Close Team to peer-owned Teams", () => {
    expect(remoteTeamMenuActions({ hasTeam: true, ownedByPeer: true, availability: "online" })).toEqual({
      canSaveTemplate: false,
      renameTeam: { visible: true, disabled: false, reason: null },
      closeTeam: { visible: true, disabled: false, reason: null },
    });
    expect(remoteTeamMenuActions({ hasTeam: true, ownedByPeer: false, availability: "online" })).toEqual({
      canSaveTemplate: false,
      renameTeam: { visible: false, disabled: true, reason: null },
      closeTeam: { visible: false, disabled: true, reason: null },
    });
    expect(remoteTeamMenuActions({ hasTeam: false, ownedByPeer: false, availability: "online" })).toEqual({
      canSaveTemplate: false,
      renameTeam: { visible: false, disabled: true, reason: null },
      closeTeam: { visible: false, disabled: true, reason: null },
    });
    for (const [availability, label] of UNAVAILABLE_STATES) {
      expect(remoteTeamMenuActions({ hasTeam: true, ownedByPeer: true, availability })).toEqual({
        canSaveTemplate: false,
        renameTeam: { visible: true, disabled: true, reason: label },
        closeTeam: { visible: true, disabled: true, reason: label },
      });
    }
  });

  it("never offers Close on protected remote tabs and disables Close when peer is unavailable", () => {
    expect(remoteTabCloseAction({ isProtected: true, availability: "online" })).toEqual({
      visible: false,
      disabled: true,
      reason: null,
    });
    expect(remoteTabCloseAction({ isProtected: false, availability: "online", remoteUnavailableReason: null })).toEqual({
      visible: true,
      disabled: false,
      reason: null,
    });
    for (const [availability, label] of UNAVAILABLE_STATES) {
      expect(remoteTabCloseAction({ isProtected: false, availability })).toEqual({
        visible: true,
        disabled: true,
        reason: label,
      });
    }
    expect(remoteTabCloseAction({
      isProtected: false,
      availability: "online",
      remoteUnavailableReason: "not paired",
    })).toEqual({
      visible: true,
      disabled: true,
      reason: "not paired",
    });
  });

  it("keeps Close active on read-only online tabs and disables Close in unavailableTabMenu when allowClose is false", () => {
    const standard = buildTabMenu({
      tab: remoteTab(),
      surface: "sidebar",
      status: null,
      descriptor: null,
      descriptors: [],
      activity: null,
      orchestrators: [],
      canRename: true,
      canClose: true,
      pluginTabMenu: { items: [], receiverTargets: {} },
      telegramThread: null,
    });
    const onlineReadOnly = unavailableTabMenu(standard, "Open this tab to use its actions.", { allowClose: true });
    const onlineClose = onlineReadOnly.find(entry => entry.id === "close");
    expect(onlineClose?.kind === "item" && onlineClose.action).toEqual({ type: "close" });
    expect(onlineClose?.kind === "item" && onlineClose.unavailable).toBeUndefined();

    const offlineMenu = unavailableTabMenu(standard, "update required", { allowClose: false });
    const offlineClose = offlineMenu.find(entry => entry.id === "close");
    expect(offlineClose?.kind === "item" && offlineClose.action).toBeNull();
    expect(offlineClose?.kind === "item" && offlineClose.unavailable).toBe(true);
    expect(offlineClose?.kind === "item" && offlineClose.hint).toBe("update required");

    const protectedMenu = buildTabMenu({
      tab: remoteTab({ protected: true }),
      surface: "sidebar",
      status: null,
      descriptor: null,
      descriptors: [],
      activity: null,
      orchestrators: [],
      canRename: true,
      canClose: false,
      pluginTabMenu: { items: [], receiverTargets: {} },
      telegramThread: null,
    });
    expect(protectedMenu.some(entry => entry.id === "close")).toBe(false);
  });

  it("derives workspace names from POSIX and Windows paths", () => {
    expect(workspaceNameFromPath("/home/user/projects/demo/")).toBe("demo");
    expect(workspaceNameFromPath("C:\\Developer\\domios\\")).toBe("domios");
    expect(workspaceNameFromPath("/")).toBe("Workspace");
  });

  it("governs remote workspace menu actions (new session, new terminal, rename, close) by peer availability and folder presence", () => {
    expect(remoteWorkspaceMenuActions({ workspaceProtected: false, cwdMissing: false, availability: "online" })).toEqual({
      newSession: { visible: true, disabled: false, reason: null },
      newTerminal: { visible: true, disabled: false, reason: null },
      renameWorkspace: { visible: true, disabled: false, reason: null },
      closeWorkspace: { visible: true, disabled: false, reason: null },
    });
    expect(remoteWorkspaceMenuActions({ workspaceProtected: false, cwdMissing: true, availability: "online" })).toEqual({
      newSession: { visible: false, disabled: true, reason: null },
      newTerminal: { visible: false, disabled: true, reason: null },
      renameWorkspace: { visible: true, disabled: false, reason: null },
      closeWorkspace: { visible: true, disabled: false, reason: null },
    });
    for (const [availability, label] of UNAVAILABLE_STATES) {
      expect(remoteWorkspaceMenuActions({ workspaceProtected: false, availability })).toEqual({
        newSession: { visible: true, disabled: true, reason: label },
        newTerminal: { visible: true, disabled: true, reason: label },
        renameWorkspace: { visible: true, disabled: true, reason: label },
        closeWorkspace: { visible: true, disabled: true, reason: label },
      });
    }
  });

  it("keeps rename, restart, stop, and close operable on online observed remote tabs without opening them", () => {
    const standard = buildTabMenu({
      tab: remoteTab({ agent_session_id: "sess-1", agent_state: { state: "working", status_line: "Working" } }),
      surface: "sidebar",
      status: "Working",
      descriptor: null,
      descriptors: [],
      activity: null,
      orchestrators: [],
      canRename: true,
      canClose: true,
      pluginTabMenu: { items: [], receiverTargets: {} },
      telegramThread: null,
    });
    const observedOnline = unavailableTabMenu(standard, "Open this tab to use its actions.", {
      allowClose: true,
      allowIds: remoteObservedTabAllowedMenuIds(true),
    });
    for (const id of ["rename", "restart", "stop", "close"]) {
      const entry = observedOnline.find(item => item.id === id);
      expect(entry?.kind === "item" && entry.action).not.toBeNull();
      expect(entry?.kind === "item" && entry.unavailable).toBeUndefined();
    }
    const observedOffline = unavailableTabMenu(standard, "offline", {
      allowClose: false,
      allowIds: remoteObservedTabAllowedMenuIds(false),
    });
    for (const id of ["rename", "restart", "stop", "close"]) {
      const entry = observedOffline.find(item => item.id === id);
      expect(entry?.kind === "item" && entry.action).toBeNull();
      expect(entry?.kind === "item" && entry.unavailable).toBe(true);
    }
  });
});
