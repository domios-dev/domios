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
