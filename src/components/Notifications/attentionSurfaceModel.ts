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
