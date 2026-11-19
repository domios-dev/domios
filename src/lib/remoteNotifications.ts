import { cleanMachineName } from "./machineIdentity";
import { invoke } from "./bridge";
import type { Notification } from "./types";
import type { AnswerResult } from "./notificationAnswerSend";
import type { NotificationAnswer, RemoteNotificationOrigin } from "./notificationTypes";
import { parseCompositeTabId } from "./remoteTabTransport";
import { adoptStoredActions } from "./notificationActions";

export type RemoteNotificationAction = "read" | "delete" | "resolve" | "answer";

export function remoteNotificationId(peerId: string, sourceId: string): string {
  requireRoutePart(peerId, "installation");
  requireRoutePart(sourceId, "notification");
  return "remote-notification/" + encodeURIComponent(peerId) + "/" + encodeURIComponent(sourceId);
}

export function remoteNotificationRoute(notification: Pick<Notification, "id" | "tab_id" | "remote_origin">): RemoteNotificationOrigin | null {
  const origin = notification.remote_origin;
  if (!isRemoteOrigin(origin)) return null;
  if (!origin.peerId.trim() || !origin.sourceId.trim()) return null;
  if (notification.id !== tryRemoteNotificationId(origin.peerId, origin.sourceId)) return null;
  if (notification.tab_id != null) {
    if (typeof notification.tab_id !== "string") return null;
    const tab = parseCompositeTabId(notification.tab_id);
    if (!tab || tab.peerId !== origin.peerId) return null;
  }
  return origin;
}

export function projectRemoteNotification(peerId: string, peerName: string, source: Notification): Notification {
  requireRoutePart(peerId, "installation");
  const sourceId = requireRoutePart(source.id, "notification");
  const answer = adoptAnswer(source.answer);
  const machine = cleanMachineName(peerName, peerId) ?? "Machine";
  return {
    id: remoteNotificationId(peerId, sourceId),
    tab_id: compositeTabId(peerId, source.tab_id),
    workspace_id: null,
    target_tab_id: compositeTabId(peerId, source.target_tab_id),
    ...(typeof source.tab_title === "string" ? { tab_title: source.tab_title } : {}),
    title: source.title,
    body: source.body,
    severity: source.severity,
    ...(isNotifyLevel(source.level) ? { level: source.level } : {}),
    timestamp: source.timestamp,
    read: source.read,
    actions: adoptStoredActions(source.actions),
    ...(answer ? { answer } : {}),
    ...(typeof source.resolved === "boolean" ? { resolved: source.resolved } : {}),
    ...(typeof source.sticky === "boolean" ? { sticky: source.sticky } : {}),
    peer_name: machine,
    remote_origin: { peerId, sourceId, peerName: machine },
  };
}

export async function loadRemoteNotifications(peerId: string, signal?: AbortSignal): Promise<Notification[]> {
  requireRoutePart(peerId, "installation");
  const rows = await invoke<unknown>("mesh_peer_notifications_list", { peerId, signal });
  return Array.isArray(rows) ? rows as Notification[] : [];
}

export async function readRemoteNotification(notification: Notification): Promise<void> {
  await actOnRemoteNotification(notification, "read");
}

export async function deleteRemoteNotification(notification: Notification): Promise<void> {
  await actOnRemoteNotification(notification, "delete");
}

export async function resolveRemoteNotification(notification: Notification): Promise<void> {
  await actOnRemoteNotification(notification, "resolve");
}

export function answerRemoteNotification(notification: Notification, label: string): Promise<AnswerResult> {
  return actOnRemoteNotification(notification, "answer", label);
}

export function remoteNotificationFromPayload(peerId: string, peerName: string, value: unknown): Notification | null {
  if (typeof value !== "object" || value === null) return null;
  const source = value as Partial<Notification>;
  if (typeof source.id !== "string" || typeof source.title !== "string"
    || (source.severity !== "Info" && source.severity !== "Warning" && source.severity !== "Error")) return null;
  try {
    return projectRemoteNotification(peerId, peerName, {
      id: source.id,
      title: source.title,
      severity: source.severity,
      body: typeof source.body === "string" ? source.body : null,
      timestamp: typeof source.timestamp === "number" && Number.isFinite(source.timestamp) ? source.timestamp : Date.now(),
      read: source.read === true,
      tab_id: typeof source.tab_id === "string" ? source.tab_id : null,
      workspace_id: typeof source.workspace_id === "string" ? source.workspace_id : null,
      target_tab_id: typeof source.target_tab_id === "string" ? source.target_tab_id : null,
      actions: source.actions,
      answer: adoptAnswer(source.answer),
      level: source.level,
      resolved: source.resolved === true,
      tab_title: typeof source.tab_title === "string" ? source.tab_title : undefined,
      sticky: source.sticky === true,
    } as Notification);
  } catch {
    return null;
  }
}

async function actOnRemoteNotification(
  notification: Notification,
  action: RemoteNotificationAction,
  label?: string,
): Promise<AnswerResult> {
  const origin = remoteNotificationRoute(notification);
  if (!origin) throw new Error("Remote notification origin is missing or invalid");
  if (action === "answer" && !label?.trim()) throw new Error("Notification answer label is required");
  return invoke<AnswerResult>("mesh_peer_notification_action", {
    peerId: origin.peerId,
    sourceId: origin.sourceId,
    notificationAction: action,
    ...(action === "answer" ? { label } : {}),
  });
}

function compositeTabId(peerId: string, tabId: string | null | undefined): string | null {
  if (!tabId) return null;
  const candidate = "mesh:" + peerId + "/" + tabId;
  return parseCompositeTabId(candidate)?.peerId === peerId ? candidate : null;
}

function requireRoutePart(value: string, label: string): string {
  if (!value.trim()) throw new Error("Remote notification " + label + " is required");
  return value;
}

function isNotifyLevel(value: unknown): value is Notification["level"] {
  return value === "verbose" || value === "action_needed" || value === "failure";
}

function tryRemoteNotificationId(peerId: string, sourceId: string): string | null {
  try {
    return remoteNotificationId(peerId, sourceId);
  } catch {
    return null;
  }
}

function isRemoteOrigin(value: unknown): value is RemoteNotificationOrigin {
  if (typeof value !== "object" || value === null) return false;
  const origin = value as Partial<RemoteNotificationOrigin>;
  return typeof origin.peerId === "string"
    && typeof origin.sourceId === "string"
    && typeof origin.peerName === "string";
}

function adoptAnswer(value: unknown): NotificationAnswer | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const candidate = value as Record<string, unknown>;
  if (!(candidate.asked_from_tab === null || typeof candidate.asked_from_tab === "string")
    || !(candidate.answered === null || typeof candidate.answered === "string")
    || !(candidate.answered_via === null || typeof candidate.answered_via === "string")
    || typeof candidate.delivered !== "boolean") return undefined;
  return {
    asked_from_tab: candidate.asked_from_tab,
    answered: candidate.answered,
    answered_via: candidate.answered_via,
    delivered: candidate.delivered,
    ...(typeof candidate.answered_by === "string" ? { answered_by: candidate.answered_by } : {}),
    ...(typeof candidate.daemon_request === "string" ? { daemon_request: candidate.daemon_request } : {}),
  };
}
