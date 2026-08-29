import type { Notification } from "../../lib/types";

export type DeclaredAttentionLevel = "action_needed" | "failure";

function isDeclaredAttentionLevel(
  level: Notification["level"],
): level is DeclaredAttentionLevel {
  return level === "action_needed" || level === "failure";
}

export function declaredAttentionByTab(
  notifications: readonly Notification[],
): Readonly<Record<string, DeclaredAttentionLevel>> {
  const attention: Record<string, { level: DeclaredAttentionLevel; timestamp: number }> = {};
  for (const notification of notifications) {
    if (
      notification.read
      || !notification.tab_id
      || !isDeclaredAttentionLevel(notification.level)
    ) {
      continue;
    }
    const current = attention[notification.tab_id];
    if (!current || notification.timestamp >= current.timestamp) {
      attention[notification.tab_id] = {
        level: notification.level,
        timestamp: notification.timestamp,
      };
    }
  }
  return Object.fromEntries(
    Object.entries(attention).map(([tabId, declaration]) => [tabId, declaration.level]),
  );
}

export function declaredAttentionLabel(level: DeclaredAttentionLevel): string {
  return level === "failure"
    ? "Agent declared a failure and stopped"
    : "Agent declared that action is needed";
}
