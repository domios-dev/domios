import type { Notification, Tab, TabStatus } from "../../lib/types";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { tabActivity } from "../../lib/tabActivity";
import { isLiveAttention } from "../Notifications/attentionSurfaceModel";
import { isAnswerUndeliverable } from "../../lib/notificationAnswer";
import { asleepLabel } from "../../lib/tabSleep";
import type { PlanControlLoop } from "../Canvas/planControlLoop";

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
  sleeping: boolean;
  sleepingLabel?: string;
  curatorSchedule?: { kind: "scheduled" | "suspended"; label: string };
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
    sleeping: tab.frozen === true,
    sleepingLabel: tab.frozen ? asleepLabel(tab, Date.now()) : undefined,
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
  | { kind: "starting" | "sleeping" | "scheduled"; label: string };

export function tabStatusMark(title: string, attention: TabAttention | null | undefined, active = false): TabStatusMark | null {
  if (!attention) return null;
  if (attention.curatorSchedule?.kind === "suspended") return { kind: "attention", label: attention.curatorSchedule.label, level: "action_needed" };
  if (attention.curatorSchedule?.kind === "scheduled" && (!attention.showMark || active)) return { kind: attention.curatorSchedule.kind, label: attention.curatorSchedule.label };
  if (attention.sleeping) return { kind: "sleeping", label: attention.sleepingLabel ?? "Asleep" };
  if (attention.showMark && !active) return { kind: "attention", label: tabAttentionLabel(title, attention), level: attention.kind };
  if (attention.starting) return { kind: "starting", label: `${title} ${attention.starting === "connecting" ? "waiting to connect" : "starting"}` };
  return null;
}

export function curatorTabAttention(attention: TabAttention, loop: PlanControlLoop | undefined): TabAttention {
  if (!loop || loop.mode !== "curator") return attention;
  const base = { ...attention, sleeping: false, starting: loop.running ? null : attention.starting };
  if (loop.suspended_reason) return {
    ...base,
    curatorSchedule: { kind: "suspended", label: loop.suspended_reason === "waiting_on_human"
      ? "Curator suspended: waiting on your answer" : `Curator suspended: ${loop.suspended_reason.replace(/_/g, " ")}` },
  };
  if (!loop.running && loop.next_at != null && !loop.fallback_reason) return {
    ...base, starting: null,
    curatorSchedule: { kind: "scheduled", label: "Curator waiting for the next round" },
  };
  return base;
}

export function tabStartingLabel(tab: Tab): string {
  const attention = tabAttention(tab, null, []);
  return tabStatusMark(displayTitleForTab(tab), {
    ...attention,
    showMark: false,
    starting: tab.boot_stage === "launch_binding_unconfirmed" ? "connecting" : "starting",
  })!.label;
}
