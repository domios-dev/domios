import type { Notification, Tab, TabStatus } from "../../lib/types";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { tabActivity } from "../../lib/tabActivity";
import { isLiveAttention } from "../Notifications/attentionSurfaceModel";
import { isAnswerUndeliverable } from "../../lib/notificationAnswer";

export type TabAttentionKind = "waiting" | "action_needed" | "failure";

export interface TabAttention {
  wantsAttention: boolean;
  kind: TabAttentionKind | null;
  reason: string;
  body: string | null;
  notification: Notification | null;
  notificationIds: string[];
  cleared: boolean;
  showMark: boolean;
  starting: "starting" | "connecting" | null;
}

export function attentionNotifications(tabId: string, notifications: readonly Notification[]): Notification[] {
  return notifications.filter((item) => item.tab_id === tabId && !item.read && isLiveAttention(item)
    && (item.level === "failure" || item.level === "action_needed"));
}

export function tabAttention(tab: Tab, status: TabStatus | null | undefined, notifications: readonly Notification[], active = false): TabAttention {
  const candidates = attentionNotifications(tab.id, notifications);
  const notification = [...candidates].sort((a, b) =>
    Number(b.level === "failure") - Number(a.level === "failure") || b.timestamp - a.timestamp)[0] ?? null;
  const activity = tabActivity(tab, status);
  const waiting = activity.kind === "waiting-on-user";
  const kind = notification ? notification.level === "failure" ? "failure" : "action_needed" : waiting ? "waiting" : null;
  return {
    wantsAttention: kind !== null,
    kind,
    reason: notification?.title ?? (waiting ? activity.label : ""),
    body: notification?.body ?? null,
    notification,
    notificationIds: candidates.map((item) => item.id),
    cleared: candidates.length === 0 && (!waiting || active),
    showMark: kind !== null && !active,
    starting: activity.kind === "starting"
      ? tab.boot_stage === "launch_binding_unconfirmed" ? "connecting" : "starting"
      : null,
  };
}

export function tabAttentionByTab(tabs: Readonly<Record<string, Tab>>, statuses: Readonly<Record<string, TabStatus>>, notifications: readonly Notification[]): Readonly<Record<string, TabAttention>> {
  return Object.fromEntries(Object.values(tabs).map((tab) => [tab.id, tabAttention(tab, statuses[tab.id], notifications)]));
}

export function tabAttentionLabel(title: string, attention: TabAttention): string {
  return `${title} needs your input: ${attention.reason}`;
}

export function attentionActions(notification: Notification | null) {
  if (!notification || notification.resolved || notification.answer?.answered
    || (notification.answer && isAnswerUndeliverable(notification.answer))) return [];
  return notification.actions ?? [];
}

export type TabStatusMark = { kind: "attention"; label: string; level: TabAttentionKind | null }
  | { kind: "starting"; label: string };

export function tabStatusMark(title: string, attention: TabAttention | null | undefined, active = false): TabStatusMark | null {
  if (!attention) return null;
  if (attention.showMark && !active) return { kind: "attention", label: tabAttentionLabel(title, attention), level: attention.kind };
  if (attention.starting) return { kind: "starting", label: `${title} ${attention.starting === "connecting" ? "waiting to connect" : "starting"}` };
  return null;
}

export function tabStartingLabel(tab: Tab): string {
  const attention = tabAttention(tab, null, []);
  return tabStatusMark(displayTitleForTab(tab), {
    ...attention,
    showMark: false,
    starting: tab.boot_stage === "launch_binding_unconfirmed" ? "connecting" : "starting",
  })!.label;
}
