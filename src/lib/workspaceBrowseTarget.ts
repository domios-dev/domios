import { useSyncExternalStore } from "react";

export interface WorkspaceBrowseTarget {
  id: string;
  name: string;
}

let target: WorkspaceBrowseTarget | null = null;
let revision = 0;
let switchOrigin: { x: number; y: number } | undefined;
const listeners = new Set<() => void>();

export function workspaceBrowseTarget() {
  return target;
}

export function workspaceBrowseRevision() {
  return revision;
}

export function workspaceSwitchOrigin() {
  return switchOrigin;
}

export function clearWorkspaceSwitchOrigin() {
  switchOrigin = undefined;
}

export function discardWorkspaceSwitchOrigin(failed: WorkspaceBrowseTarget) {
  if (target === failed) switchOrigin = undefined;
}

export function setWorkspaceBrowseTarget(next: WorkspaceBrowseTarget | null, origin?: { x: number; y: number }) {
  if (target?.id === next?.id && target?.name === next?.name) return;
  switchOrigin = target?.id !== next?.id ? origin : undefined;
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
