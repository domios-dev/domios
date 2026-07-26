import { useCallback, useRef } from "react";
import { invoke } from "../../lib/bridge";

export interface TranscriptAuthority {
  source_session_id: string | null;
  session_binding_nonce: string | null;
}

export type ChatAttachmentState =
  | "idle"
  | "attaching"
  | "awaiting_session"
  | "awaiting_store"
  | "attached"
  | "failed";

export function canAdoptTranscriptAuthority(
  incoming: TranscriptAuthority,
  paneSessionId: string | null,
  paneSessionBindingNonce: string | null,
  allowUnboundAuthority = false,
): boolean {
  if (paneSessionId == null && !allowUnboundAuthority) return false;
  return incoming.source_session_id === paneSessionId
    && (incoming.session_binding_nonce ?? null) === paneSessionBindingNonce;
}

let subscriptionSequence = 0;

export function newChatSubscriptionId(paneId: string): string {
  const randomId = globalThis.crypto?.randomUUID?.();
  if (randomId) return randomId;
  subscriptionSequence += 1;
  return `${paneId}-${subscriptionSequence}`;
}

type SubscriptionEdge = "owned" | "released";

export function useChatSubscription(
  paneId: string,
  onEdge: (edge: SubscriptionEdge, subscriptionId: string) => void,
) {
  const currentIdRef = useRef<string | null>(null);

  const stop = useCallback((subscriptionId: string) => {
    onEdge("released", subscriptionId);
    void invoke("chat_stop", { paneId, subscriptionId }).catch(() => {});
  }, [onEdge, paneId]);

  const replace = useCallback((subscriptionId: string | null) => {
    const previousSubscriptionId = currentIdRef.current;
    currentIdRef.current = subscriptionId;
    if (subscriptionId && subscriptionId !== previousSubscriptionId) {
      onEdge("owned", subscriptionId);
    }
    if (previousSubscriptionId && previousSubscriptionId !== subscriptionId) {
      stop(previousSubscriptionId);
    }
  }, [onEdge, stop]);

  const stopUnadopted = useCallback((subscriptionId: string | null | undefined) => {
    if (subscriptionId && subscriptionId !== currentIdRef.current) stop(subscriptionId);
  }, [stop]);

  const release = useCallback(() => {
    const subscriptionId = currentIdRef.current;
    currentIdRef.current = null;
    if (subscriptionId) stop(subscriptionId);
  }, [stop]);

  return { release, replace, stopUnadopted };
}
