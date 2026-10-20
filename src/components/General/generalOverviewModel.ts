import { isUtilityShell } from "../../lib/shellCommandState";
import type { Tab, Team, Workspace } from "../../lib/types";
import { jobWorkspace, type MergedJob } from "../Jobs/jobsModel";
import { buildInventoryModel, DEFAULT_INVENTORY_FILTERS, type InventorySnapshot } from "../MeshPanel/meshInventoryModel";

export interface UtilityShellRow {
  tab: Tab;
  owner: { id: string; name: string } | null;
  team: { id: string; name: string } | null;
  workspace: { id: string; name: string };
  target: { workspaceId: string; tabId: string };
}

export function projectUtilityShells(workspaces: readonly Workspace[], tabs: Record<string, Tab>, teams: readonly Team[], workspaceId?: string): UtilityShellRow[] {
  return workspaces.filter((workspace) => workspaceId === undefined || workspace.id === workspaceId).flatMap((workspace) => workspace.tabs.flatMap((id): UtilityShellRow[] => {
    const tab = tabs[id];
    if (!tab || tab.remote_peer_id || tab.remote_tab_id || !isUtilityShell(tab)) return [];
    const ownerId = tab.parent_tab_id ?? tab.spawned_by_tab_id;
    const team = teams.find((entry) => entry.id === tab.team_id);
    return [{
      tab,
      owner: ownerId ? { id: ownerId, name: tabs[ownerId]?.title ?? ownerId } : null,
      team: tab.team_id ? { id: tab.team_id, name: team?.name ?? tab.team_id } : null,
      workspace: { id: workspace.id, name: workspace.name },
      target: { workspaceId: workspace.id, tabId: tab.id },
    }];
  }));
}

export function generalInventorySections(snapshot: InventorySnapshot, machineName: string) {
  const resources = snapshot.resources.filter((resource) => !resource.source_peer_id && !resource.source_peer && resource.kind === "daemon");
  return { services: buildInventoryModel({ ...snapshot, resources }, DEFAULT_INVENTORY_FILTERS, undefined, machineName) };
}

export function generalWorkspaceJobs(jobs: readonly MergedJob[], workspaceId: string): MergedJob[] {
  return jobs.filter((job) => jobWorkspace(job) === workspaceId);
}
