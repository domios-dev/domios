import { uid } from "../../lib/uid";
import type { Notification } from "../../lib/types";

/** The toast shown when the daemon refuses a sidebar reorder and the order snaps back. */
export function reorderFailureToast(workspaceId: string | null, error: unknown, now = Date.now()): Notification {
  return {
    id: uid(),
    timestamp: now,
    read: false,
    tab_id: null,
    workspace_id: workspaceId,
    title: "Could not save the new order",
    body: error instanceof Error ? error.message : String(error),
    severity: "Error",
    level: "failure",
  };
}
