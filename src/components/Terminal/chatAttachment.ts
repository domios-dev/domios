import { useCallback, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";

export type ChatAttachmentEdge = "owned" | "released";

let subscriptionSequence = 0;

function newSubscriptionId(tabId: string): string {
  const randomId = globalThis.crypto?.randomUUID?.();
  if (randomId) return randomId;
  subscriptionSequence += 1;
  return `${tabId}-${subscriptionSequence}`;
}

export interface ChatAttachment {
  /** Render-visible generation; changing it is what requests a snapshot. */
  generation: number;
  begin: () => void;
  isCurrent: (generation: number) => boolean;
  requestedSubscriptionId: () => string;
  adopt: (subscriptionId: string | null | undefined) => void;
  discardForeign: (subscriptionId: string | null | undefined) => void;
  release: () => void;
}

/** The single owner of attachment generation and subscription lifetime for one
 *  tab: one counter, one requested id, one adopted id, one release path. */
export function useChatAttachment(
  tabId: string,
  onEdge: (edge: ChatAttachmentEdge, subscriptionId: string) => void,
): ChatAttachment {
  const [generation, setGeneration] = useState(0);
  const generationRef = useRef(generation);
  const requestedIdRef = useRef<string | null>(null);
  const adoptedIdRef = useRef<string | null>(null);
  const onEdgeRef = useRef(onEdge);
  onEdgeRef.current = onEdge;

  const stop = useCallback((subscriptionId: string) => {
    onEdgeRef.current("released", subscriptionId);
    void invoke("chat_stop", { tabId, subscriptionId }).catch(() => {});
  }, [tabId]);

  const begin = useCallback(() => {
    requestedIdRef.current = newSubscriptionId(tabId);
    setGeneration((previous) => {
      const next = previous + 1;
      generationRef.current = next;
      return next;
    });
  }, [tabId]);

  const isCurrent = useCallback((candidate: number) => generationRef.current === candidate, []);

  const requestedSubscriptionId = useCallback(() => {
    const existing = requestedIdRef.current ?? newSubscriptionId(tabId);
    requestedIdRef.current = existing;
    return existing;
  }, [tabId]);

  const adopt = useCallback((subscriptionId: string | null | undefined) => {
    const previous = adoptedIdRef.current;
    const next = subscriptionId ?? null;
    adoptedIdRef.current = next;
    if (next && next !== previous) onEdgeRef.current("owned", next);
    if (previous && previous !== next) stop(previous);
  }, [stop]);

  const discardForeign = useCallback((subscriptionId: string | null | undefined) => {
    if (subscriptionId && subscriptionId !== adoptedIdRef.current) stop(subscriptionId);
  }, [stop]);

  const release = useCallback(() => {
    const subscriptionId = adoptedIdRef.current;
    adoptedIdRef.current = null;
    if (subscriptionId) stop(subscriptionId);
  }, [stop]);

  return { generation, begin, isCurrent, requestedSubscriptionId, adopt, discardForeign, release };
}
