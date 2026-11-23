import { createContext, useMemo, useSyncExternalStore } from "react";
import { setWorkspaceBrowseTarget, useWorkspaceBrowseTarget, type WorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";

export type SidebarMachineFilter = "all" | "local" | { peerId: string };

export const SidebarWholeTeam = createContext(false);
export const SidebarWholeWorkspace = createContext(false);

let localOnly = true;
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

const readLocalOnly = () => localOnly;

export function setSidebarMachineFilter(selection: "all" | "local" | WorkspaceBrowseTarget, origin?: { x: number; y: number }): void {
  const local = selection === "local";
  if (typeof selection === "string" && localOnly !== local) {
    localOnly = local;
    for (const listener of listeners) listener();
  }
  setWorkspaceBrowseTarget(typeof selection === "object" ? selection : null, origin);
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

export function sidebarWorkspaceKey(peerId: string | null, workspaceId: string): string {
  return JSON.stringify([peerId, workspaceId]);
}

export function groupSidebarWorkspaceMachines<T extends { peerId: string | null }>(
  workspaces: readonly T[],
  peerIds: readonly (string | null)[] = [],
) {
  const groups = new Map<string | null, T[]>();
  for (const workspace of workspaces) {
    const entries = groups.get(workspace.peerId) ?? [];
    entries.push(workspace);
    groups.set(workspace.peerId, entries);
  }
  for (const peerId of peerIds) if (!groups.has(peerId)) groups.set(peerId, []);
  return [...groups].sort(([left], [right]) => left === null ? -1 : right === null ? 1 : left < right ? -1 : left > right ? 1 : 0)
    .map(([peerId, entries]) => ({ peerId, workspaces: entries }));
}

export function selectSidebarWorkspaceView<T extends { key: string; peerId: string | null; selected?: boolean }>(
  workspaces: readonly T[],
  selectedKey: string | null,
  filter: SidebarMachineFilter,
) {
  const visible = workspaces.filter(workspace => matchesSidebarMachine(filter, workspace.peerId));
  return {
    visible,
    selected: visible.find(workspace => workspace.key === selectedKey)
      ?? visible.find(workspace => workspace.selected)
      ?? visible[0] ?? null,
  };
}
