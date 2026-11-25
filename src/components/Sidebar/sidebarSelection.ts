import { remoteViewerIdentity } from "../../lib/remoteTabPortal";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { isConciergeTab } from "../../lib/tabInputPolicy";
import type { Tab, Workspace } from "../../lib/types";
import type {
  BrowsedWorkspace,
  PairedGeneral,
  PeerLastActiveSelection,
} from "../../store/multiplexer/pairedWorkspaceProjection";
import { sidebarWorkspaceKey, type SidebarMachineFilter } from "./sidebarMachineFilter";
import { isSidebarTabVisible } from "./workspaceTabs";

/** Sidebar workspace surface: exactly one of plan entry or session tab is visually active. */

export function workspacePlanTabId(workspaceId: string): string {
  return `workspace-plan:${workspaceId}`;
}

export function isWorkspacePlanEntryActive(
  workspaceIsActiveVisual: boolean,
  workspaceCanvasActive: boolean,
): boolean {
  return workspaceIsActiveVisual && workspaceCanvasActive;
}

export function isWorkspaceTabActive(
  workspaceIsActiveVisual: boolean,
  workspaceCanvasActive: boolean,
  tabId: string,
  activeTabId: string | undefined,
): boolean {
  if (!workspaceIsActiveVisual || workspaceCanvasActive) return false;
  return tabId === activeTabId;
}

export function resolveSidebarActiveTabId(
  isWorkspaceMode: boolean,
  workspaceCanvasActive: boolean,
  activeWorkspaceId: string | null,
  activeTabIds: Readonly<Record<string, string>>,
): string | null {
  if (!isWorkspaceMode || workspaceCanvasActive || !activeWorkspaceId) return null;
  return activeTabIds[activeWorkspaceId] ?? null;
}

export function isGeneralSectionOwningTab(
  activeWorkspaceId: string | null,
  generalWorkspaceId: string | null,
  activeTabId: string | null,
  tabs: Readonly<Record<string, Tab>>,
  observedGeneralViewerIds: ReadonlySet<string>,
): boolean {
  if (!generalWorkspaceId || !activeTabId || activeWorkspaceId !== generalWorkspaceId) return false;
  if (observedGeneralViewerIds.has(activeTabId)) return true;
  const tab = tabs[activeTabId];
  return Boolean(tab && (!remoteViewerIdentity(tab) || tab.team_id));
}

export function resolveObservedWorkspaceSelectedViewerId(
  selectedViewerId: string | null,
  visibleViewers: { has(id: string): boolean },
  ownedByGeneral = false,
): string | null {
  if (ownedByGeneral || !selectedViewerId) return null;
  return visibleViewers.has(selectedViewerId) ? selectedViewerId : null;
}

export interface LocalTabSelection {
  workspaceId: string;
  tabId: string;
}

export interface MachineSwitchSelection {
  workspaceId: string;
  tabId: string;
  preferredWorkspaceKey?: string;
}

export function isLocalSidebarTab(
  tabId: string | null | undefined,
  tabs: Readonly<Record<string, Tab>>,
): boolean {
  if (!tabId || parseCompositeTabId(tabId)) return false;
  const tab = tabs[tabId];
  return Boolean(tab && !tab.remote_peer_id);
}

function isSelectableLocalTab(
  workspace: Workspace,
  tabId: string,
  tabs: Readonly<Record<string, Tab>>,
): boolean {
  if (!workspace.tabs.includes(tabId) || parseCompositeTabId(tabId)) return false;
  const tab = tabs[tabId];
  return Boolean(tab && !tab.remote_peer_id && isSidebarTabVisible(tab) && !isConciergeTab(tab));
}

export function resolveLocalMachineTab(
  lastLocal: LocalTabSelection | null,
  workspaces: readonly Workspace[],
  tabs: Readonly<Record<string, Tab>>,
): LocalTabSelection | null {
  if (lastLocal) {
    const rememberedWorkspace = workspaces.find(workspace => workspace.id === lastLocal.workspaceId);
    if (rememberedWorkspace) {
      if (isSelectableLocalTab(rememberedWorkspace, lastLocal.tabId, tabs)) return lastLocal;
      if (rememberedWorkspace.active_tab_id
        && isSelectableLocalTab(rememberedWorkspace, rememberedWorkspace.active_tab_id, tabs)) {
        return { workspaceId: rememberedWorkspace.id, tabId: rememberedWorkspace.active_tab_id };
      }
      const firstInWorkspace = rememberedWorkspace.tabs.find(id => isSelectableLocalTab(rememberedWorkspace, id, tabs));
      if (firstInWorkspace) return { workspaceId: rememberedWorkspace.id, tabId: firstInWorkspace };
    }
  }
  const general = workspaces.find(workspace => workspace.protected);
  if (general) {
    const protectedMain = general.tabs.find(id => isSelectableLocalTab(general, id, tabs) && tabs[id]?.protected);
    if (protectedMain) return { workspaceId: general.id, tabId: protectedMain };
    if (general.active_tab_id && isSelectableLocalTab(general, general.active_tab_id, tabs)) {
      return { workspaceId: general.id, tabId: general.active_tab_id };
    }
    const firstGeneral = general.tabs.find(id => isSelectableLocalTab(general, id, tabs));
    if (firstGeneral) return { workspaceId: general.id, tabId: firstGeneral };
  }
  for (const workspace of workspaces) {
    if (workspace.active_tab_id && isSelectableLocalTab(workspace, workspace.active_tab_id, tabs)) {
      return { workspaceId: workspace.id, tabId: workspace.active_tab_id };
    }
    const first = workspace.tabs.find(id => isSelectableLocalTab(workspace, id, tabs));
    if (first) return { workspaceId: workspace.id, tabId: first };
  }
  return null;
}

export function resolvePeerMachineTab(
  peerId: string,
  generals: readonly PairedGeneral[],
  peerWorkspaces: readonly BrowsedWorkspace[],
  attachedTabs: Readonly<Record<string, Tab>>,
): PeerLastActiveSelection | null {
  const general = generals.find(candidate => candidate.peerId === peerId);
  const workspaces = peerWorkspaces.filter(workspace => workspace.peerId === peerId);
  const generalSessions = general?.sessions ?? [];
  const isOpenable = (viewerId: string, inGeneral: boolean) => {
    const attached = attachedTabs[viewerId];
    if (attached && remoteViewerIdentity(attached)?.peerId === peerId) return true;
    if (inGeneral) return Boolean(general && !general.cached);
    return general?.availability === "online";
  };
  const findWorkspaceSession = (viewerId: string) => {
    for (const workspace of workspaces) {
      const match = workspace.tabs.find(session =>
        !session.peerId && !isConciergeTab(session.tab) && session.viewerId === viewerId);
      if (match) return { viewerId: match.viewerId, workspaceId: workspace.workspace.id };
    }
    return null;
  };
  if (general?.lastActiveTab) {
    const target = general.lastActiveTab;
    if (target.workspaceId === null) {
      if (generalSessions.some(session => session.viewerId === target.viewerId) && isOpenable(target.viewerId, true)) {
        return target;
      }
    } else {
      const found = findWorkspaceSession(target.viewerId);
      if (found && isOpenable(found.viewerId, false)) return found;
    }
  } else if (general?.lastActiveTab === undefined) {
    const selectedWorkspace = workspaces.find(workspace => workspace.selected);
    const activeRawId = selectedWorkspace?.workspace.active_tab_id;
    if (selectedWorkspace && activeRawId) {
      const match = selectedWorkspace.tabs.find(session =>
        !session.peerId && !isConciergeTab(session.tab) && session.tab.id === activeRawId);
      if (match && isOpenable(match.viewerId, false)) {
        return { viewerId: match.viewerId, workspaceId: selectedWorkspace.workspace.id };
      }
    }
  }
  const main = generalSessions.find(session => session.tab.protected) ?? generalSessions[0];
  if (main && isOpenable(main.viewerId, true)) {
    return { viewerId: main.viewerId, workspaceId: null };
  }
  return null;
}

export function resolveMachineSwitchSelection({
  filter,
  activeWorkspaceId,
  activeTabId,
  generalWorkspaceId,
  lastLocal,
  workspaces,
  tabs,
  generals,
  peerWorkspaces,
}: {
  filter: SidebarMachineFilter;
  activeWorkspaceId: string | null;
  activeTabId: string | null;
  generalWorkspaceId: string | null;
  lastLocal: LocalTabSelection | null;
  workspaces: readonly Workspace[];
  tabs: Readonly<Record<string, Tab>>;
  generals: readonly PairedGeneral[];
  peerWorkspaces: readonly BrowsedWorkspace[];
}): MachineSwitchSelection | null {
  if (filter === "all") return null;
  if (filter === "local") {
    if (activeWorkspaceId && isLocalSidebarTab(activeTabId, tabs)) return null;
    const local = resolveLocalMachineTab(lastLocal, workspaces, tabs);
    if (!local) return null;
    const workspace = workspaces.find(candidate => candidate.id === local.workspaceId);
    return {
      workspaceId: local.workspaceId,
      tabId: local.tabId,
      preferredWorkspaceKey: workspace && !workspace.protected
        ? sidebarWorkspaceKey(null, workspace.id)
        : undefined,
    };
  }
  const { peerId } = filter;
  const viewerWorkspaceId = generalWorkspaceId ?? activeWorkspaceId ?? workspaces[0]?.id ?? null;
  if (!viewerWorkspaceId) return null;
  if (activeTabId && parseCompositeTabId(activeTabId)?.peerId === peerId) {
    const general = generals.find(candidate => candidate.peerId === peerId);
    if (general?.sessions.some(session => session.viewerId === activeTabId)) return null;
    const owningWorkspace = peerWorkspaces.find(workspace => workspace.peerId === peerId
      && workspace.tabs.some(session => !session.peerId && !isConciergeTab(session.tab) && session.viewerId === activeTabId));
    if (owningWorkspace) {
      return {
        workspaceId: activeWorkspaceId ?? viewerWorkspaceId,
        tabId: activeTabId,
        preferredWorkspaceKey: sidebarWorkspaceKey(peerId, owningWorkspace.workspace.id),
      };
    }
  }
  const peerTarget = resolvePeerMachineTab(peerId, generals, peerWorkspaces, tabs);
  if (!peerTarget) return null;
  return {
    workspaceId: viewerWorkspaceId,
    tabId: peerTarget.viewerId,
    preferredWorkspaceKey: peerTarget.workspaceId
      ? sidebarWorkspaceKey(peerId, peerTarget.workspaceId)
      : undefined,
  };
}
