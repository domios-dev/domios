import type { Notification } from "../../lib/types";
import { localFetch } from "../../lib/webApi";

/** Stable daemon refusal code for a free-tier limit; the text after it is already user-facing. */
export const FREE_LIMIT_REACHED = "free_limit_reached";

export function freeLimitMessage(error: unknown): string | null {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  const marker = `${FREE_LIMIT_REACHED}: `;
  const at = text.indexOf(marker);
  return at < 0 ? null : text.slice(at + marker.length).trim() || null;
}

/** One toast per workspace that replaces itself; null when the error is not a free-tier refusal. */
export function freeLimitToast(
  error: unknown,
  workspaceId: string | null,
  upgrade: () => Promise<void>,
  now = Date.now(),
): Notification | null {
  const body = freeLimitMessage(error);
  if (!body) return null;
  return {
    id: `${FREE_LIMIT_REACHED}:${workspaceId ?? "app"}`,
    timestamp: now,
    read: false,
    tab_id: null,
    workspace_id: workspaceId,
    title: "Free version limit reached",
    body,
    severity: "Warning",
    level: "action_needed",
    sticky: true,
    local_actions: [{ label: "Upgrade", run: upgrade }],
  };
}

/** The backend checks out commercial keys only, never the bare plan name. */
export const UPGRADE_PLAN_KEY = "pro_monthly";

export async function upgradeToPro(): Promise<void> {
  const response = await localFetch("/api/account/upgrade", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ plan: UPGRADE_PLAN_KEY }),
  });
  if (!response.ok) throw new Error(await response.text());
}
