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

export function ensureProtectedExpanded(expanded: Set<string>, workspaces: Workspace[]): Set<string> {
  const next = new Set(expanded);
  for (const workspace of workspaces) {
    if (workspace.protected) next.add(workspace.id);
  }
  return next;
}
