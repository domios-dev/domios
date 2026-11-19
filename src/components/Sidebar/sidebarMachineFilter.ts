import { createContext, useMemo, useSyncExternalStore } from "react";
import { setWorkspaceBrowseTarget, useWorkspaceBrowseTarget, type WorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";

export type SidebarMachineFilter = "all" | "local" | { peerId: string };

export const SidebarWholeTeam = createContext(false);

let localOnly = true;
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

const readLocalOnly = () => localOnly;

export function setSidebarMachineFilter(selection: "all" | "local" | WorkspaceBrowseTarget): void {
  const local = selection === "local";
  if (typeof selection === "string" && localOnly !== local) {
    localOnly = local;
    for (const listener of listeners) listener();
  }
  setWorkspaceBrowseTarget(typeof selection === "object" ? selection : null);
}

export function useSidebarMachineFilter(): SidebarMachineFilter {
  const browse = useWorkspaceBrowseTarget();
  const local = useSyncExternalStore(subscribe, readLocalOnly, readLocalOnly);
  return useMemo(() => browse ? { peerId: browse.id } : local ? "local" : "all", [browse, local]);
}

export function matchesSidebarMachine(filter: SidebarMachineFilter, peerId?: string | null): boolean {
  if (filter === "all") return true;
  if (filter === "local") return !peerId;
  return filter.peerId === peerId;
}

export function wholeSidebarTeam<T>(filter: SidebarMachineFilter, members: readonly T[], peerOf: (member: T) => string | null | undefined): readonly T[] {
  return members.some(member => matchesSidebarMachine(filter, peerOf(member))) ? members : [];
}

export function selectSidebarWorkspaceView<T extends { id: string }>(
  workspaces: readonly T[],
  selectedId: string | null,
  filter: SidebarMachineFilter,
  memberPeers: (workspace: T) => readonly (string | null | undefined)[],
) {
  const visible = filter === "all" ? [...workspaces] : workspaces.filter(workspace =>
    memberPeers(workspace).some(peerId => matchesSidebarMachine(filter, peerId)));
  return {
    visible,
    selected: visible.find(workspace => workspace.id === selectedId) ?? visible[0] ?? null,
  };
}
