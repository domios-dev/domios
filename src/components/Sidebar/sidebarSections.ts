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
