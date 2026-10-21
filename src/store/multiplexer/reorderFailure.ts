import { uid } from "../../lib/uid";
import type { Notice } from "../../lib/notices";

/** The toast shown when the daemon refuses a sidebar reorder and the order snaps back. */
export function reorderFailureToast(workspaceId: string | null, error: unknown): Notice {
  return {
    kind: "toast",
    id: uid(),
    tabId: null,
    workspaceId: workspaceId,
    title: "Could not save the new order",
    detail: error instanceof Error ? error.message : String(error),
    severity: "Error",
  };
}
