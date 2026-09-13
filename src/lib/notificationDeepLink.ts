import type { ChatRevealRequest } from "./chatMessageReveal";
import type { Notification, Tab, Workspace } from "./types";

export type DeepLinkUnavailableReason = "no_target" | "tab_gone" | "message_gone";

export type DeepLinkState =
  | { kind: "available"; tabId: string; workspaceId: string; messageId: string }
  | { kind: "unavailable"; reason: DeepLinkUnavailableReason };

export const DEEP_LINK_UNAVAILABLE_COPY: Record<DeepLinkUnavailableReason, string> = {
  no_target: "unavailable: no chat message was recorded",
  tab_gone: "unavailable: the target tab is closed",
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
  return notification.level === "action_needed" && !notification.read;
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
  const workspaceId = workspaces.find((ws) => ws.tabs.includes(tabId))?.id ?? tabs[tabId]?.workspace_id ?? null;
  if (!tabs[tabId] || !workspaceId) return { kind: "unavailable", reason: "tab_gone" };
  const refused = reveal?.notificationId === notification.id
    && reveal.tabId === tabId
    && reveal.messageId === messageId
    && reveal.outcome === "missing";
  if (refused) return { kind: "unavailable", reason: "message_gone" };
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
