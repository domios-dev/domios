import { useSyncExternalStore } from "react";

export interface WorkspaceBrowseTarget {
  id: string;
  name: string;
}

let target: WorkspaceBrowseTarget | null = null;
let revision = 0;
const listeners = new Set<() => void>();

export function workspaceBrowseTarget() {
  return target;
}

export function workspaceBrowseRevision() {
  return revision;
}

export function setWorkspaceBrowseTarget(next: WorkspaceBrowseTarget | null) {
  if (target?.id === next?.id && target?.name === next?.name) return;
  target = next;
  revision++;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useWorkspaceBrowseTarget() {
  return useSyncExternalStore(subscribe, workspaceBrowseTarget, workspaceBrowseTarget);
}
