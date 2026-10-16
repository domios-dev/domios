import { isUtilityShell } from "../../lib/shellCommandState";
import type { Tab, Team, Workspace } from "../../lib/types";
import { buildInventoryModel, DEFAULT_INVENTORY_FILTERS, type InventorySnapshot } from "../MeshPanel/meshInventoryModel";

export interface UtilityShellRow {
  tab: Tab;
  owner: { id: string; name: string } | null;
  team: { id: string; name: string } | null;
  workspace: { id: string; name: string };
  target: { workspaceId: string; tabId: string };
}

export function projectUtilityShells(workspaces: readonly Workspace[], tabs: Record<string, Tab>, teams: readonly Team[]): UtilityShellRow[] {
  return workspaces.flatMap((workspace) => workspace.tabs.flatMap((id): UtilityShellRow[] => {
    const tab = tabs[id];
    if (!tab || !isUtilityShell(tab)) return [];
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
  const schedules = buildInventoryModel(snapshot, { ...DEFAULT_INVENTORY_FILTERS, kind: "cron" }, undefined, machineName);
  return {
    services: buildInventoryModel(snapshot, { ...DEFAULT_INVENTORY_FILTERS, kind: "daemon" }, undefined, machineName),
    schedules,
    remoteSchedules: schedules.rows.filter((row) => row.resource.source_peer_id || row.resource.source_peer),
  };
}
