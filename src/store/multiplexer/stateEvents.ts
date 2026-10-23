import { WebApiError } from "../../lib/webApi";
import type { TabBootStageEvent } from "../../lib/types";
import type { Action } from "./reducer";

export const STORAGE_UNAVAILABLE_MESSAGE =
  "Storage unavailable, your data is preserved, see the incident.";

export function isStorageUnavailableError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return message.includes(STORAGE_UNAVAILABLE_MESSAGE);
}

/** The daemon refuses snapshots under load or while lifecycle locks are held.
 * Distinct from a failure: the state is fine, the request was declined and a
 * state event will schedule the next read. */
export function isSnapshotRefusal(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return (
    (error instanceof DOMException && error.name === "AbortError") ||
    (error instanceof WebApiError && error.status === 503 && message.includes("state snapshot")) ||
    message.includes("state snapshot unavailable while tab state is initializing or changing") ||
    message.includes("tab summary unavailable while tab state is initializing or changing")
  );
}

export function muxStateEventAction(payload: unknown): "apply-snapshot" | "refresh" {
  if (
    payload &&
    typeof payload === "object" &&
    Array.isArray((payload as { workspaces?: unknown }).workspaces)
  ) {
    return "apply-snapshot";
  }
  return "refresh";
}

export function tabBootStageEventAction(payload: TabBootStageEvent): Action {
  return {
    type: "UPDATE_BOOT_STAGE",
    tabId: payload.tab_id,
    stage: payload.stage,
    reason: payload.reason,
    agent_booting: payload.agent_booting,
    session_bound: payload.session_bound,
    provisional_launch: payload.provisional_launch,
    launchGeneration: payload.launch_generation,
  };
}

