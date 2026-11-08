import { useMemo, useSyncExternalStore } from "react";
import { AutomationResource, type AutomationSnapshot } from "./resource";
import type { AutomationAttachment, AutomationGraphAdapter } from "./types";

const DETACHED: AutomationSnapshot = { graph: null, loading: false, failed: false, stopping: null, stopFailed: null };
const detached = () => DETACHED;
const noSubscription = () => () => undefined;

/** The attached scope's graph, for a surface that keeps one element shape whether or not its scope is known yet. */
export function useAttachedAutomationSnapshot(adapter: AutomationGraphAdapter, attachment: AutomationAttachment | null): AutomationSnapshot {
  const { connection_id, workspace_id, team_id } = attachment ?? {};
  const resource = useMemo(() => attachment && new AutomationResource(adapter, attachment), [adapter, connection_id, workspace_id, team_id]);
  const getSnapshot = resource?.getSnapshot ?? detached;
  return useSyncExternalStore(resource?.subscribe ?? noSubscription, getSnapshot, getSnapshot);
}

export function useAutomationGraph(adapter: AutomationGraphAdapter, attachment: AutomationAttachment) {
  const { connection_id, workspace_id, team_id } = attachment;
  const resource = useMemo(() => new AutomationResource(adapter, { connection_id, workspace_id, team_id }), [adapter, connection_id, workspace_id, team_id]);
  return { ...useSyncExternalStore(resource.subscribe, resource.getSnapshot, resource.getSnapshot), stop: resource.stop, refresh: resource.refresh };
}
