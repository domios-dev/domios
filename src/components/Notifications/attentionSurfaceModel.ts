import type { Notification } from "../../lib/types";

export function isLiveAttention(notification: Notification): boolean {
  return !notification.resolved
    && (notification.answer === undefined
      || (notification.answer.answered === null && notification.answer.asked_from_tab !== null));
}

export function retainedAttention(
  notifications: readonly Notification[], visitedId: string | null,
): Notification | undefined {
  return notifications.find((item) => item.id === visitedId && isLiveAttention(item));
}

export function attentionSurfaceLayout(labels: readonly string[], width: number, expanded = false) {
  const actionsWidth = labels.reduce((total, label) => total + label.length * 8 + 28, 0);
  const inlineActions = labels.length > 0 && actionsWidth <= Math.max(0, width - 240);
  return { inlineActions, showChoose: labels.length > 0 && !inlineActions, showDetails: expanded };
}
