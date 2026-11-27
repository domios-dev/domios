import { useCallback } from "react";
import { invoke } from "../../lib/bridge";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { authoredCause } from "../../lib/describeError";
import type { Notice } from "../../lib/notices";
import type { RemotePeerTabs } from "../../lib/remoteTabPortal";
import { createOrchestrationTransport } from "../../lib/transport/orchestration";
import { createTabsTransport } from "../../lib/transport/tabs";
import type { Tab } from "../../lib/types";
import type {
  BrowsedWorkspace,
  GeneralAvailability,
  PairedGeneral,
} from "../../store/multiplexer/pairedWorkspaceProjection";
import { remotePeerActionEligibility } from "./remoteSidebarActionsModel";
import type {
  RemoteWorkspaceCloseRequestResult,
  RemoteWorkspaceCloseResult,
  RemoteWorkspaceCloseTarget,
} from "./useRemoteWorkspaceClose";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function peerRefusalReason(raw: string): string {
  return raw.replace(/^.*? → HTTP \d{3}: /, "").trim() || raw.trim();
}

export function resolveRemoteWorkspaceForShortcut(
  peerId: string,
  activeTab: Pick<Tab, "remote_peer_id" | "remote_workspace_id"> | null | undefined,
  peerWorkspaces?: readonly BrowsedWorkspace[] | null,
): BrowsedWorkspace | null {
  const candidates = (peerWorkspaces ?? []).filter((workspace) => workspace.peerId === peerId && !workspace.workspace.protected);
  if (activeTab?.remote_peer_id === peerId && activeTab.remote_workspace_id) {
    const activeMatch = candidates.find((workspace) => workspace.workspace.id === activeTab.remote_workspace_id);
    if (activeMatch) return activeMatch;
  }
  return candidates.find((workspace) => workspace.selected) ?? candidates[0] ?? null;
}

export function findObservedTabOwnerWorkspaceId(
  viewerTabId: string,
  peerWorkspaces?: readonly BrowsedWorkspace[] | null,
  generals?: readonly PairedGeneral[] | null,
): string | null {
  for (const workspace of peerWorkspaces ?? []) {
    const match = workspace.tabs.find((session) => session.viewerId === viewerTabId);
    if (match) {
      if (!match.peerId) return workspace.workspace.id;
      if (match.tab.remote_workspace_id) return match.tab.remote_workspace_id;
    }
  }
  for (const peer of generals ?? []) {
    const match = peer.sessions.find((session) => session.viewerId === viewerTabId);
    if (match?.tab.workspace_id) return match.tab.workspace_id;
  }
  return null;
}

export interface RemoteWorkspaceManagementPort {
  renameWorkspace(peerId: string, workspaceId: string, name: string): Promise<void>;
  renameTeam(peerId: string, teamId: string, name: string): Promise<void>;
  createTerminalTab(
    peerId: string,
    workspaceId: string,
    cwd: string | null | undefined,
    shell?: string,
  ): Promise<{ id: string; title: string }>;
  listPeerTabs(peerId: string): Promise<RemotePeerTabs>;
}

export const remoteWorkspaceManagementPort: RemoteWorkspaceManagementPort = {
  async renameWorkspace(peerId, workspaceId, name) {
    const transport = createTabsTransport(daemonTransportForPeer(peerId));
    await transport.mux_rename_workspace({ workspaceId, name });
  },
  async renameTeam(peerId, teamId, name) {
    const transport = createOrchestrationTransport(daemonTransportForPeer(peerId));
    await transport.mux_rename_team({ teamId, name });
  },
  async createTerminalTab(peerId, workspaceId, cwd, shell) {
    const transport = createTabsTransport(daemonTransportForPeer(peerId));
    const result = await transport.mux_create_tab({
      workspaceId,
      cwd: cwd || undefined,
      shell,
      activate: false,
    }) as { id?: string; tab_id?: string; title?: string } | null;
    const id = result?.id ?? result?.tab_id;
    if (typeof id !== "string" || !id.trim()) {
      throw new Error("The selected machine did not return the created terminal tab.");
    }
    return { id, title: typeof result?.title === "string" && result.title.trim() ? result.title : "Terminal" };
  },
  listPeerTabs(peerId) {
    return invoke<RemotePeerTabs>("mesh_peer_tabs_list", { peerId });
  },
};

export function remoteWorkspaceActionErrorNotice({
  id,
  workspaceId,
  title,
  machineName,
  cause,
  detail,
  unavailableReason,
}: {
  id: string;
  workspaceId: string;
  title: string;
  machineName: string;
  cause: string;
  detail?: string;
  unavailableReason?: string;
}): Notice {
  return {
    kind: "toast",
    id,
    tabId: null,
    workspaceId,
    title,
    detail: unavailableReason
      ? authoredCause(`${machineName} is ${unavailableReason}.`)
      : JSON.stringify({
        code: "workspace_close_refused",
        message: peerRefusalReason(detail ?? cause),
        cause,
        raw: detail ? `${machineName}\n${detail}` : machineName,
      }),
    severity: "Error",
    persistent: true,
    retainInNotifications: true,
  };
}

export function useRemoteWorkspaceManagement({
  availabilityForPeer,
  machineNameForPeer,
  generalWorkspaceId,
  attachRemoteTab,
  refreshPeerInventory,
  notify,
  port = remoteWorkspaceManagementPort,
}: {
  availabilityForPeer: (peerId: string) => GeneralAvailability | undefined;
  machineNameForPeer: (peerId: string) => string;
  generalWorkspaceId: string | null;
  attachRemoteTab?: (workspaceId: string, peer: RemotePeerTabs, tab: Tab) => void;
  refreshPeerInventory?: (peerId: string) => void;
  notify: (notice: Notice) => void;
  port?: RemoteWorkspaceManagementPort;
}) {
  const renameRemoteWorkspace = useCallback(async (peerId: string, workspaceId: string, name: string): Promise<boolean> => {
    const trimmed = name.trim();
    if (!trimmed) return false;
    const machineName = machineNameForPeer(peerId);
    const eligibility = remotePeerActionEligibility(availabilityForPeer(peerId));
    if (!eligibility.allowed) {
      notify(remoteWorkspaceActionErrorNotice({
        id: `remote-workspace-rename:${peerId}:${workspaceId}`,
        workspaceId,
        title: "Workspace rename not sent",
        machineName,
        cause: `Could not rename the workspace on ${machineName}.`,
        unavailableReason: eligibility.reason ?? "offline",
      }));
      return false;
    }
    try {
      await port.renameWorkspace(peerId, workspaceId, trimmed);
      refreshPeerInventory?.(peerId);
      return true;
    } catch (error) {
      notify(remoteWorkspaceActionErrorNotice({
        id: `remote-workspace-rename:${peerId}:${workspaceId}`,
        workspaceId,
        title: "Workspace rename failed",
        machineName,
        cause: `Could not rename the workspace on ${machineName}.`,
        detail: errorMessage(error),
      }));
      return false;
    }
  }, [availabilityForPeer, machineNameForPeer, notify, port, refreshPeerInventory]);

  const createRemoteTerminalTab = useCallback(async (
    peerId: string,
    workspaceId: string,
    cwd: string | null | undefined,
    shell?: string,
  ): Promise<{ tabId: string } | null> => {
    const machineName = machineNameForPeer(peerId);
    const eligibility = remotePeerActionEligibility(availabilityForPeer(peerId));
    if (!eligibility.allowed) {
      notify(remoteWorkspaceActionErrorNotice({
        id: `remote-terminal-create:${peerId}:${workspaceId}`,
        workspaceId,
        title: "Terminal not started",
        machineName,
        cause: `Could not open a terminal on ${machineName}.`,
        unavailableReason: eligibility.reason ?? "offline",
      }));
      return null;
    }
    try {
      const created = await port.createTerminalTab(peerId, workspaceId, cwd, shell);
      refreshPeerInventory?.(peerId);
      if (generalWorkspaceId && attachRemoteTab) {
        let peer: RemotePeerTabs = { peer_id: peerId, peer_name: machineName, tabs: [] };
        let tab: Tab = {
          id: created.id,
          pty_id: created.id,
          tab_type: "Terminal",
          title: created.title,
          cwd: cwd ?? null,
          workspace_id: workspaceId,
        };
        try {
          const listed = await port.listPeerTabs(peerId);
          const match = listed.tabs.find((candidate) => candidate.id === created.id);
          if (match) {
            peer = listed;
            tab = match.workspace_id ? match : { ...match, workspace_id: workspaceId };
          }
        } catch {
          // The created tab record already has enough identity to open its viewer while inventory catches up.
        }
        attachRemoteTab(generalWorkspaceId, peer, tab);
      }
      return { tabId: `mesh:${peerId}/${created.id}` };
    } catch (error) {
      notify(remoteWorkspaceActionErrorNotice({
        id: `remote-terminal-create:${peerId}:${workspaceId}`,
        workspaceId,
        title: "Terminal could not start",
        machineName,
        cause: `Could not open a terminal on ${machineName}.`,
        detail: errorMessage(error),
      }));
      return null;
    }
  }, [attachRemoteTab, availabilityForPeer, generalWorkspaceId, machineNameForPeer, notify, port, refreshPeerInventory]);

  const renameRemoteTeam = useCallback(async (peerId: string, teamId: string, name: string): Promise<void> => {
    const trimmed = name.trim();
    if (!trimmed) return;
    const machineName = machineNameForPeer(peerId);
    const eligibility = remotePeerActionEligibility(availabilityForPeer(peerId));
    if (!eligibility.allowed) {
      throw new Error(`${machineName} is ${eligibility.reason ?? "offline"}.`);
    }
    await port.renameTeam(peerId, teamId, trimmed);
    refreshPeerInventory?.(peerId);
  }, [availabilityForPeer, machineNameForPeer, port, refreshPeerInventory]);

  return { renameRemoteWorkspace, renameRemoteTeam, createRemoteTerminalTab };
}

export function remoteWorkspaceCloseInventoryNotice(
  target: RemoteWorkspaceCloseTarget,
  result: Exclude<RemoteWorkspaceCloseRequestResult, { kind: "ready" | "stale" }>,
): Notice {
  const detailText = result.kind === "inventory_failed"
    ? `${target.machineName} · ${target.workspaceName}\n${result.detail}`
    : result.kind === "protected"
      ? `${target.workspaceName} on ${target.machineName} is protected.`
      : `${target.workspaceName} on ${target.machineName} cannot be inspected while the machine is offline.`;
  const message = result.kind === "inventory_failed" ? peerRefusalReason(result.detail) : detailText;
  return {
    kind: "toast",
    id: `workspace-close-inventory:${target.peerId}:${target.workspaceId}`,
    tabId: null,
    workspaceId: target.workspaceId,
    title: result.kind === "offline" ? "Machine offline" : "Close inventory unavailable",
    detail: JSON.stringify({ code: "workspace_close_refused", message, cause: `${target.workspaceName} on ${target.machineName} stays open.`, raw: detailText }),
    severity: "Error",
    persistent: true,
    retainInNotifications: true,
  };
}

export function remoteWorkspaceCloseOutcomeNotice(
  result: Exclude<RemoteWorkspaceCloseResult, { kind: "none" }>,
): Notice {
  const target = result.target;
  const targetLabel = `${target.workspaceName} on ${target.machineName}`;
  const workspaceRefusal = (message: string, cause: string, raw: string) =>
    JSON.stringify({ code: "workspace_close_refused", message: peerRefusalReason(message), cause, raw });
  const notification = result.kind === "closed"
    ? { title: "Workspace closed", detail: targetLabel, severity: "Info" as const }
    : result.kind === "closed_with_warning"
      ? { title: "Workspace closed with a cleanup warning", detail: workspaceRefusal(result.detail, `${targetLabel} closed with a cleanup warning.`, `${targetLabel}\n${result.detail}`), severity: "Error" as const, persistent: true, retainInNotifications: true }
      : result.kind === "still_present"
        ? { title: "Workspace remains open", detail: workspaceRefusal(result.detail, `${targetLabel} stays open.`, `${targetLabel}\n${result.detail}`), severity: "Error" as const, persistent: true, retainInNotifications: true }
        : result.kind === "offline"
          ? { title: "Workspace close not sent", detail: workspaceRefusal(`${targetLabel} is offline.`, `${targetLabel} stays open.`, `${targetLabel} is offline.`), severity: "Error" as const, persistent: true, retainInNotifications: true }
          : { title: "Close status could not be checked", detail: workspaceRefusal(result.detail, `Refresh ${target.machineName} before starting another close.`, `${targetLabel}\n${result.detail}\nRefresh the machine before starting another close.`), severity: "Error" as const, persistent: true, retainInNotifications: true };
  return {
    kind: "toast",
    id: `workspace-close:${target.peerId}:${target.workspaceId}`,
    tabId: null,
    workspaceId: target.workspaceId,
    ...notification,
  };
}
