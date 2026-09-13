import { useMemo, type KeyboardEvent } from "react";
import { useChatReveal } from "../../lib/chatMessageReveal";
import { jumpToChatMessage } from "./jumpToChatMessage";
import {
  DEEP_LINK_UNAVAILABLE_COPY,
  needsYouNotificationItems,
  type DeepLinkScope,
  type NeedsYouNotificationItem,
} from "../../lib/notificationDeepLink";
import type { Notification, Tab, Workspace } from "../../lib/types";

export interface NeedsYouDeepLinks {
  notifications: Notification[];
  tabs: Record<string, Tab>;
  workspaces: Workspace[];
  scope: DeepLinkScope;
  onSelectTab: (workspaceId: string, tabId: string) => void | Promise<void>;
}

const NO_ITEMS: NeedsYouNotificationItem[] = [];

export function useNeedsYouNotificationItems(links?: NeedsYouDeepLinks): NeedsYouNotificationItem[] {
  const reveal = useChatReveal();
  return useMemo(
    () => (links
      ? needsYouNotificationItems(links.notifications, links.tabs, links.workspaces, links.scope, reveal)
      : NO_ITEMS),
    [links, reveal],
  );
}

/** The whole row is the one jump; it carries no action controls. */
export function NeedsYouNotificationRow({
  item,
  tabs,
  onSelectTab,
}: {
  item: NeedsYouNotificationItem;
  tabs: NeedsYouDeepLinks["tabs"];
  onSelectTab: NeedsYouDeepLinks["onSelectTab"];
}) {
  const { state } = item;
  const unavailable = state.kind === "unavailable" ? DEEP_LINK_UNAVAILABLE_COPY[state.reason] : null;
  const activate = () => {
    if (state.kind !== "available") return;
    const tab = tabs[state.tabId];
    if (!tab) return;
    jumpToChatMessage(
      { workspaceId: state.workspaceId, tabId: state.tabId, messageId: state.messageId, notificationId: item.id },
      tab,
      onSelectTab,
    );
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    activate();
  };
  return (
    <li
      data-testid="mc-board-needs-you-item"
      data-kind="notification"
      data-notification-id={item.id}
      data-deep-link={state.kind}
    >
      <div
        role="link"
        tabIndex={0}
        aria-disabled={unavailable ? true : undefined}
        aria-label={unavailable ? `${item.title}, ${unavailable}` : item.title}
        data-testid="needs-you-notification-link"
        className="mc-board-needs-link"
        onClick={activate}
        onKeyDown={onKeyDown}
      >
        <p className="mc-board-needs-title">{item.title}</p>
        {unavailable && (
          <p data-testid="needs-you-notification-unavailable" className="mc-board-caption">
            {unavailable}
          </p>
        )}
      </div>
    </li>
  );
}
