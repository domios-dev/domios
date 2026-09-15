import type { ChatRevealRequest } from "./chatMessageReveal";
import { tabChatCapable } from "./chatUiMode";
import type { Notification, Tab, Workspace } from "./types";

export type DeepLinkUnavailableReason = "no_target" | "no_stable_id" | "tab_gone" | "no_chat" | "message_gone";

// Offset-anchored ids name a batch origin plus a position, so the same message gets another id in another parse window.
const OFFSET_ANCHORED_ID = /^(?:result-)?\d+-\d+(?:-(?:img-)?\d+)?$/;

/** Only a provider-uuid id (`{uuid}-{block}`) names the same message in every loaded window. */
export function isStableChatMessageId(id: string): boolean {
  return id.length > 0 && !OFFSET_ANCHORED_ID.test(id);
}

export type DeepLinkState =
  | { kind: "available"; tabId: string; workspaceId: string; messageId: string }
  | { kind: "unavailable"; reason: DeepLinkUnavailableReason };

export const DEEP_LINK_UNAVAILABLE_COPY: Record<DeepLinkUnavailableReason, string> = {
  no_target: "unavailable: no chat message was recorded",
  no_stable_id: "unavailable: that message has no stable id",
  tab_gone: "unavailable: the target tab is closed",
  no_chat: "unavailable: the target tab has no chat view",
  message_gone: "unavailable: that message is not in the loaded chat",
};

export interface DeepLinkScope {
  teamId?: string | null;
  workspaceId?: string | null;
}

export interface NeedsYouNotificationItem {
  key: string;
  id: string;
  title: string;
  state: DeepLinkState;
}

export function isNeedsYouNotification(notification: Notification): boolean {
  return notification.level === "action_needed" && !notification.read && !notification.answer?.answered;
}

/** Scope reads daemon-resolved provenance only: a caller-supplied jump target cannot place a record in a team. */
export function notificationInScope(
  notification: Notification,
  tabs: Record<string, Tab>,
  scope: DeepLinkScope,
): boolean {
  if (scope.teamId) {
    return notification.tab_id != null && tabs[notification.tab_id]?.team_id === scope.teamId;
  }
  if (scope.workspaceId) return notification.workspace_id === scope.workspaceId;
  return false;
}

export function resolveDeepLink(
  notification: Notification,
  tabs: Record<string, Tab>,
  workspaces: readonly Workspace[],
  reveal: ChatRevealRequest | null = null,
): DeepLinkState {
  const tabId = notification.target_tab_id ?? null;
  const messageId = notification.chat_message_id ?? null;
  if (!tabId || !messageId) return { kind: "unavailable", reason: "no_target" };
  if (!isStableChatMessageId(messageId)) return { kind: "unavailable", reason: "no_stable_id" };
  const workspaceId = workspaces.find((ws) => ws.tabs.includes(tabId))?.id ?? tabs[tabId]?.workspace_id ?? null;
  const tab = tabs[tabId];
  if (!tab || !workspaceId) return { kind: "unavailable", reason: "tab_gone" };
  if (!tabChatCapable(tab)) return { kind: "unavailable", reason: "no_chat" };
  const settled = reveal?.notificationId === notification.id
    && reveal.tabId === tabId
    && reveal.messageId === messageId;
  if (settled && reveal?.outcome === "missing") return { kind: "unavailable", reason: "message_gone" };
  if (settled && reveal?.outcome === "no_chat") return { kind: "unavailable", reason: "no_chat" };
  return { kind: "available", tabId, workspaceId, messageId };
}

export function needsYouNotificationItems(
  notifications: readonly Notification[],
  tabs: Record<string, Tab>,
  workspaces: readonly Workspace[],
  scope: DeepLinkScope,
  reveal: ChatRevealRequest | null = null,
): NeedsYouNotificationItem[] {
  return notifications
    .filter((notification) => isNeedsYouNotification(notification) && notificationInScope(notification, tabs, scope))
    .sort((a, b) => b.timestamp - a.timestamp)
    .map((notification) => ({
      key: `notification-${notification.id}`,
      id: notification.id,
      title: notification.title,
      state: resolveDeepLink(notification, tabs, workspaces, reveal),
    }));
}
