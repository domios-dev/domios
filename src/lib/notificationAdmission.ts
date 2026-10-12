import type { Notification } from "./notificationTypes";

export type NoticeSource = "agent_notification" | "local_notice" | "plugin_progress" | "diagnostic";
export type NoticeDestination = "bell" | "toast" | "activity" | "log";

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
  const duplicate = records.some((record) => !record.resolved && !record.answer?.answered
    && record.tab_id === incoming.tab_id
    && record.workspace_id === incoming.workspace_id
    && record.title === incoming.title
    && record.body === incoming.body
    && record.severity === incoming.severity
    && record.level === incoming.level
    && record.target_tab_id === incoming.target_tab_id
    && record.chat_message_id === incoming.chat_message_id
    && JSON.stringify(record.actions) === JSON.stringify(incoming.actions));
  return duplicate ? records : [...records, incoming];
}
