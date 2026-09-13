import { canvasFocusStore } from "../Canvas/canvasFocusStore";
import { requestChatMessageReveal, settleChatMessageReveal } from "../../lib/chatMessageReveal";
import { tabChatCapable } from "../../lib/chatUiMode";
import { setClientTabUiMode } from "../../lib/clientTabUiMode";
import type { Tab } from "../../lib/types";

export interface ChatJumpTarget {
  workspaceId: string;
  tabId: string;
  messageId: string;
  notificationId: string | null;
}

/** Leaves every Board and canvas surface, opens the tab in Chat, then selects it; the chat settles the reveal. */
export function jumpToChatMessage(
  target: ChatJumpTarget,
  tab: Tab,
  selectTab: (workspaceId: string, tabId: string) => void | Promise<void>,
): void {
  const request = requestChatMessageReveal(target.tabId, target.messageId, target.notificationId);
  if (!tabChatCapable(tab)) {
    settleChatMessageReveal(request.id, "no_chat");
    return;
  }
  canvasFocusStore.close();
  canvasFocusStore.closeWorkspace();
  setClientTabUiMode(target.tabId, "chat");
  void selectTab(target.workspaceId, target.tabId);
}
