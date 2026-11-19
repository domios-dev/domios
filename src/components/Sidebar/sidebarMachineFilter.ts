import { useSyncExternalStore } from "react";
import { setWorkspaceBrowseTarget, useWorkspaceBrowseTarget, type WorkspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";

export type SidebarMachineFilter = "all" | "local" | { peerId: string };

let localOnly = false;
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

const readLocalOnly = () => localOnly;

export function setSidebarMachineFilter(selection: "all" | "local" | WorkspaceBrowseTarget): void {
  const local = selection === "local";
  if (localOnly !== local) {
    localOnly = local;
    for (const listener of listeners) listener();
  }
  setWorkspaceBrowseTarget(typeof selection === "object" ? selection : null);
}

export function useSidebarMachineFilter(): SidebarMachineFilter {
  const browse = useWorkspaceBrowseTarget();
  const local = useSyncExternalStore(subscribe, readLocalOnly, readLocalOnly);
  return browse ? { peerId: browse.id } : local ? "local" : "all";
}

export function matchesSidebarMachine(filter: SidebarMachineFilter, peerId?: string | null): boolean {
  if (filter === "all") return true;
  if (filter === "local") return !peerId;
  return filter.peerId === peerId;
}
