import type { Tab } from "./types";

export function tabWakeRequiresRetry(tab: Partial<Tab> | null | undefined): boolean {
  if (tab?.boot_stage === "spawn_failed") return true;
  try {
    const reason: unknown = JSON.parse(tab?.boot_failure_reason ?? "null");
    if (!reason || typeof reason !== "object") return false;
    const failure = reason as { class?: unknown; code?: unknown };
    return failure.class === "requires_workspace_repair" || failure.code === "workspace_cwd_missing";
  } catch { return false; }
}

export function shouldWakeOnOpen(
  tab: Partial<Tab> | null | undefined,
  isVisible: boolean,
  alreadyRequested: boolean,
): boolean {
  const missingBackend = tab?.backing?.kind === "pty" && !tab.pty_id && !tab.protected
    && !tab.close_restore?.stopped;
  return Boolean(tab && !tabWakeRequiresRetry(tab) && (tab.frozen || missingBackend) && isVisible && !alreadyRequested);
}
