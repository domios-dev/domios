import { useEffect, useRef } from "react";
import type { Notification } from "../../lib/types";
import { AutomationResource } from "./resource";
import { hostAutomationAdapter } from "./hostAdapter";
import { ALL_JOBS_SCOPE } from "./types";
import { advanceRunPopups, emptyPopupState } from "./runPopupModel";

export function useBlueprintRunPopups(workspaces: readonly { id: string }[], generation: number, toasts: readonly Notification[], upsert: (toast: Omit<Notification, "timestamp" | "read">) => void) {
  const visible = useRef(new Set<string>());
  visible.current = new Set(toasts.map((toast) => toast.id));
  const workspaceKey = JSON.stringify(workspaces.map((workspace) => workspace.id).sort());
  useEffect(() => {
    const ids: string[] = JSON.parse(workspaceKey);
    const subscriptions = ids.map((workspace_id) => {
      let model = emptyPopupState();
      const resource = new AutomationResource(hostAutomationAdapter, { connection_id: `host:${generation}`, workspace_id, team_id: ALL_JOBS_SCOPE });
      return resource.subscribe(() => {
        const snapshot = resource.getSnapshot();
        if (!snapshot.graph || snapshot.failed) return;
        const result = advanceRunPopups(model, snapshot.graph, Date.now(), visible.current);
        model = result.state;
        result.toasts.forEach((toast) => { visible.current.add(toast.id); upsert(toast); });
      });
    });
    return () => subscriptions.forEach((unsubscribe) => unsubscribe());
  }, [workspaceKey, generation, upsert]);
}
