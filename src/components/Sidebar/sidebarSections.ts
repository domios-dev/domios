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
