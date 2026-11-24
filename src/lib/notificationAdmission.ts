import type { Notification } from "./notificationTypes";

export type NoticeSource = "agent_notification" | "local_notice" | "plugin_progress" | "diagnostic";
export type NoticeDestination = "bell" | "toast" | "activity" | "log";
export const MAX_BELL_NOTIFICATIONS = 500;
export const BELL_NOTIFICATION_AGE_MS = 30 * 24 * 60 * 60 * 1000;

function appendBellNotification(records: Notification[], incoming: Notification): Notification[] {
  const appended = [...records, incoming];
  if (appended.length <= MAX_BELL_NOTIFICATIONS) return appended;
  const oldest = appended.reduce((oldest, row, index) =>
    row.timestamp < appended[oldest].timestamp ? index : oldest, 0);
  appended.splice(oldest, 1);
  return appended;
}

export function pruneBellNotifications(records: Notification[], now: number): Notification[] {
  const cutoff = now - BELL_NOTIFICATION_AGE_MS;
  return records.some(row => row.timestamp <= cutoff)
    ? records.filter(row => row.timestamp > cutoff)
    : records;
}

export function isRemoteNotification(row: Pick<Notification, "id" | "remote_origin">): boolean {
  return Boolean(row.remote_origin) || row.id.startsWith("remote-notification/");
}

export function notificationDestination(source: NoticeSource): NoticeDestination {
  switch (source) {
    case "agent_notification": return "bell";
    case "local_notice": return "toast";
    case "plugin_progress": return "activity";
    case "diagnostic": return "log";
  }
}

export function admitBellNotification(
  records: Notification[],
  incoming: Notification,
  source: NoticeSource,
): Notification[] {
  if (notificationDestination(source) !== "bell") return records;
  const sameId = records.findIndex((record) => record.id === incoming.id);
  if (sameId !== -1) return records.map((record, index) => index === sameId ? incoming : record);
  if (isRemoteNotification(incoming)) return appendBellNotification(records, incoming);
  const duplicate = records.some((record) => !isRemoteNotification(record) && !record.resolved && !record.answer?.answered
    && record.tab_id === incoming.tab_id
    && record.workspace_id === incoming.workspace_id
    && record.title === incoming.title
    && record.body === incoming.body
    && record.severity === incoming.severity
    && record.level === incoming.level
    && record.target_tab_id === incoming.target_tab_id
    && record.chat_message_id === incoming.chat_message_id
    && JSON.stringify(record.actions) === JSON.stringify(incoming.actions));
  return duplicate ? records : appendBellNotification(records, incoming);
}
