import { useLayoutEffect } from "react";
import { settleChatMessageReveal, useChatReveal } from "../../lib/chatMessageReveal";

export interface UseChatMessageRevealOptions {
  tabId: string;
  /** The transcript is visible and no longer loading, so an absent row is really absent. */
  ready: boolean;
  contentVersion: unknown;
  revealMessage: (messageId: string) => boolean;
  prepareMessage?: (messageId: string) => boolean;
}

/** Settles a pending jump for this tab by exact message id; there is no text fallback. */
export function useChatMessageReveal({ tabId, ready, contentVersion, revealMessage, prepareMessage }: UseChatMessageRevealOptions): void {
  const request = useChatReveal();
  useLayoutEffect(() => {
    if (!request || request.outcome !== "pending" || request.tabId !== tabId || !ready) return;
    if (revealMessage(request.messageId)) settleChatMessageReveal(request.id, "revealed");
    else if (!prepareMessage?.(request.messageId)) settleChatMessageReveal(request.id, "missing");
  }, [request, tabId, ready, contentVersion, revealMessage, prepareMessage]);
}
