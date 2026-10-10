import { useMemo, useSyncExternalStore } from "react";
import { AutomationResource, type AutomationSnapshot } from "../Automation/resource";
import { ALL_JOBS_SCOPE, type AutomationAttachment, type AutomationGraphAdapter } from "../Automation/types";

export interface WorkspaceGraph extends AutomationSnapshot {
  attachment: AutomationAttachment;
  stop: AutomationResource["stop"];
  refresh: AutomationResource["refresh"];
}

class WorkspaceGraphSet {
  readonly entries: ReadonlyArray<{ attachment: AutomationAttachment; resource: AutomationResource }>;
  private cached: readonly AutomationSnapshot[] = [];

  constructor(adapter: AutomationGraphAdapter, connectionId: string, workspaceIds: readonly string[]) {
    this.entries = workspaceIds.map((workspace_id) => {
      const attachment = { connection_id: connectionId, workspace_id, team_id: ALL_JOBS_SCOPE };
      return { attachment, resource: new AutomationResource(adapter, attachment) };
    });
  }

  subscribe = (listener: () => void) => {
    const stops = this.entries.map(({ resource }) => resource.subscribe(listener));
    return () => { for (const stop of stops) stop(); };
  };

  getSnapshot = () => {
    const next = this.entries.map(({ resource }) => resource.getSnapshot());
    if (next.length !== this.cached.length || next.some((snapshot, index) => snapshot !== this.cached[index])) this.cached = next;
    return this.cached;
  };
}

/** One all-Teams graph subscription per workspace; Team filtering happens on the merged job list. */
export function useWorkspaceGraphs(adapter: AutomationGraphAdapter, connectionId: string, workspaceIds: readonly string[]): readonly WorkspaceGraph[] {
  const key = workspaceIds.join("\n");
  const set = useMemo(() => new WorkspaceGraphSet(adapter, connectionId, workspaceIds), [adapter, connectionId, key]);
  const snapshots = useSyncExternalStore(set.subscribe, set.getSnapshot, set.getSnapshot);
  return useMemo(() => set.entries.map(({ attachment, resource }, index) => ({
    ...snapshots[index], attachment, stop: resource.stop, refresh: resource.refresh,
  })), [set, snapshots]);
}
