export const OPEN_NOTIFICATION_CENTER_EVENT = "codeterm:open-notification-center";

/** Opens the titlebar bell's panel from anywhere, such as the command palette. */
export function openNotificationCenter(notificationId?: string): void {
  window.dispatchEvent(new CustomEvent(OPEN_NOTIFICATION_CENTER_EVENT, { detail: { notificationId } }));
}
