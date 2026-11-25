import { useCallback, useEffect, useRef } from "react";
import type { Tab, Workspace } from "../../lib/types";
import type { BrowsedWorkspace, PairedGeneral } from "../../store/multiplexer/pairedWorkspaceProjection";
import { sidebarWorkspaceKey, type SidebarMachineFilter } from "./sidebarMachineFilter";
import {
  isLocalSidebarTab,
  resolveMachineSwitchSelection,
  type LocalTabSelection,
} from "./sidebarSelection";

function filterKey(filter: SidebarMachineFilter): string {
  if (typeof filter === "string") return filter;
  return `peer:${filter.peerId}`;
}

export function useSidebarMachineSelection({
  machineFilter,
  activeWorkspaceId,
  activeTabId,
  activeOrdinaryWorkspaceId,
  generalWorkspaceId,
  workspaces,
  tabs,
  generals,
  peerWorkspaces,
  browse,
  selectedWorkspaceKey,
  setPreferredWorkspaceKey,
  setActiveTab,
}: {
  machineFilter: SidebarMachineFilter;
  activeWorkspaceId: string | null;
  activeTabId: string | null;
  activeOrdinaryWorkspaceId: string | undefined;
  generalWorkspaceId: string | null;
  workspaces: readonly Workspace[];
  tabs: Readonly<Record<string, Tab>>;
  generals: readonly PairedGeneral[];
  peerWorkspaces: readonly BrowsedWorkspace[];
  browse: { target: { id: string }; synced?: boolean } | null;
  selectedWorkspaceKey: string | null;
  setPreferredWorkspaceKey: (key: string | null) => void;
  setActiveTab: (workspaceId: string, tabId: string) => void;
}) {
  const lastLocalRef = useRef<LocalTabSelection | null>(null);
  const lastLocalOrdinaryKeyRef = useRef<string | null>(null);
  const currentFilterKey = filterKey(machineFilter);
  const previousFilterKeyRef = useRef(currentFilterKey);
  const pendingSwitchKeyRef = useRef<string | null>(null);

  if (activeWorkspaceId && activeTabId && isLocalSidebarTab(activeTabId, tabs)) {
    lastLocalRef.current = { workspaceId: activeWorkspaceId, tabId: activeTabId };
  }
  if (activeOrdinaryWorkspaceId) {
    lastLocalOrdinaryKeyRef.current = sidebarWorkspaceKey(null, activeOrdinaryWorkspaceId);
  } else if (machineFilter === "local" && selectedWorkspaceKey?.startsWith("[null,")) {
    lastLocalOrdinaryKeyRef.current = selectedWorkspaceKey;
  }

  if (previousFilterKeyRef.current !== currentFilterKey) {
    previousFilterKeyRef.current = currentFilterKey;
    pendingSwitchKeyRef.current = currentFilterKey === "all" ? null : currentFilterKey;
  }

  const cancelPendingSwitch = useCallback(() => {
    pendingSwitchKeyRef.current = null;
  }, []);

  useEffect(() => {
    if (pendingSwitchKeyRef.current !== currentFilterKey) return;
    if (typeof machineFilter === "object"
      && browse?.target.id === machineFilter.peerId
      && browse.synced === false) {
      return;
    }
    const next = resolveMachineSwitchSelection({
      filter: machineFilter,
      activeWorkspaceId,
      activeTabId,
      generalWorkspaceId,
      lastLocal: lastLocalRef.current,
      workspaces,
      tabs,
      generals,
      peerWorkspaces,
    });
    pendingSwitchKeyRef.current = null;
    if (!next) return;
    if (next.preferredWorkspaceKey) {
      setPreferredWorkspaceKey(next.preferredWorkspaceKey);
    } else if (machineFilter === "local" && lastLocalOrdinaryKeyRef.current) {
      setPreferredWorkspaceKey(lastLocalOrdinaryKeyRef.current);
    }
    if (next.tabId !== activeTabId || next.workspaceId !== activeWorkspaceId) {
      setActiveTab(next.workspaceId, next.tabId);
    }
  }, [
    currentFilterKey,
    machineFilter,
    browse,
    activeWorkspaceId,
    activeTabId,
    generalWorkspaceId,
    workspaces,
    tabs,
    generals,
    peerWorkspaces,
    setPreferredWorkspaceKey,
    setActiveTab,
  ]);

  return { cancelPendingSwitch };
}
