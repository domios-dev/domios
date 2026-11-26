import type { Workspace } from "../../lib/types";

export function partitionSidebarWorkspaces(workspaces: Workspace[]): {
  general: Workspace | null;
  workspaces: Workspace[];
} {
  return {
    general: workspaces.find((workspace) => workspace.protected) ?? null,
    workspaces: workspaces.filter((workspace) => !workspace.protected),
  };
}

export function initialSidebarExpanded(workspaces: Workspace[], activeWorkspaceId: string | null): Set<string> {
  return new Set([
    ...workspaces.filter((workspace) => workspace.protected).map((workspace) => workspace.id),
    ...(activeWorkspaceId ? [activeWorkspaceId] : []),
  ]);
}

export function hydrateProtectedExpanded(
  expanded: Set<string>,
  hydratedProtectedIds: Set<string>,
  workspaces: Workspace[],
): { expanded: Set<string>; hydratedProtectedIds: Set<string> } {
  const next = new Set(expanded);
  const hydrated = new Set(hydratedProtectedIds);
  for (const workspace of workspaces) {
    if (workspace.protected && !hydrated.has(workspace.id)) {
      next.add(workspace.id);
      hydrated.add(workspace.id);
    }
  }
  return { expanded: next, hydratedProtectedIds: hydrated };
}


export interface ScrollVerticalBounds {
  top: number;
  bottom: number;
}

export function minimalScrollDeltaForItem(
  viewport: ScrollVerticalBounds,
  item: ScrollVerticalBounds,
): number {
  const viewportHeight = viewport.bottom - viewport.top;
  const itemHeight = item.bottom - item.top;
  if (viewportHeight <= 0 || itemHeight <= 0) return 0;
  if (item.top < viewport.top) return item.top - viewport.top;
  if (item.bottom > viewport.bottom) return item.bottom - viewport.bottom;
  return 0;
}

export function minimalScrollTopForItem(
  currentScrollTop: number,
  viewport: ScrollVerticalBounds,
  item: ScrollVerticalBounds,
): number {
  const delta = minimalScrollDeltaForItem(viewport, item);
  return delta === 0 ? currentScrollTop : Math.max(0, currentScrollTop + delta);
}
