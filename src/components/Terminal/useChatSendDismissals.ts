import { useEffect } from "react";
import { listen } from "../../lib/bridge";
import { observeChatSendDismissal, type ChatSendDismissal } from "./chatSendStore";

export function useChatSendDismissals(tabId: string): void {
  useEffect(() => {
    let closed = false;
    let release: (() => void) | undefined;
    void listen<{ tab_id: string; receipt: ChatSendDismissal }>("chat-send-dismissed", ({ payload }) => {
      if (!closed && payload.tab_id === tabId) observeChatSendDismissal(tabId, payload.receipt);
    }).then((unlisten) => {
      if (closed) unlisten();
      else release = unlisten;
    }).catch(() => {});
    return () => { closed = true; release?.(); };
  }, [tabId]);
}
