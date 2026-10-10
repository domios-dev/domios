import { useMemo, useSyncExternalStore } from "react";
import { AutomationResource } from "./resource";
import type { AutomationAttachment, AutomationGraphAdapter } from "./types";

export function useAutomationGraph(adapter: AutomationGraphAdapter, attachment: AutomationAttachment) {
  const { connection_id, workspace_id, team_id } = attachment;
  const resource = useMemo(() => new AutomationResource(adapter, { connection_id, workspace_id, team_id }), [adapter, connection_id, workspace_id, team_id]);
  return { ...useSyncExternalStore(resource.subscribe, resource.getSnapshot, resource.getSnapshot), stop: resource.stop, refresh: resource.refresh };
}
