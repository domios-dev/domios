import { uid } from "../../lib/uid";
import type { Notice } from "../../lib/notices";
import { describeError } from "../../lib/describeError";

/** The toast shown when the daemon refuses a sidebar reorder and the order snaps back. */
export function reorderFailureToast(workspaceId: string | null, error: unknown): Notice {
  const description = describeError({ code: "reorder_failed", raw: describeError(error).raw });
  return {
    kind: "toast",
    id: uid(),
    tabId: null,
    workspaceId: workspaceId,
    title: description.title,
    detail: JSON.stringify({ code: "reorder_failed", message: description.cause }),
    severity: "Error",
  };
}
