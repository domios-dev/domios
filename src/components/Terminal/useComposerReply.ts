import { useCallback, useEffect, useMemo, useRef } from "react";
import { usePersistentDraft } from "../../lib/usePersistentDraft";
import { contextFileName } from "./composerInsertion";
import {
  composerReplyStore,
  decodeComposerReply,
  encodeComposerReply,
  sameComposerReply,
  type ComposerReply,
} from "./composerReply";
import { storeBlob } from "./voice/audio";

/** The composer's docked reply: Quote requests replace it, send and Remove clear it. */
export function useComposerReply(tabId: string, onError: (message: string) => void, onDocked: () => void, keepInline = false) {
  const [raw, setRaw] = usePersistentDraft(`tab-session-chat-reply:${tabId}`, onError);
  const reply = useMemo(() => decodeComposerReply(raw), [raw]);
  const replyRef = useRef<ComposerReply | null>(reply);
  replyRef.current = reply;
  const callbacks = useRef({ onError, onDocked });
  callbacks.current = { onError, onDocked };
  const keepInlineRef = useRef(keepInline);
  keepInlineRef.current = keepInline;
  const generation = useRef(0);

  const set = useCallback((next: ComposerReply | null) => {
    generation.current += 1;
    replyRef.current = next;
    setRaw(encodeComposerReply(next));
  }, [setRaw]);

  const clear = useCallback(() => set(null), [set]);

  /** Clears only the reply that was sent, so a newer Quote made while sending survives. */
  const clearIfSent = useCallback((sent: ComposerReply | null) => {
    if (sent && sameComposerReply(replyRef.current, sent)) set(null);
  }, [set]);

  useEffect(() => {
    const drain = () => {
      const request = composerReplyStore.take(tabId);
      if (!request) return;
      if (keepInlineRef.current) {
        set({ label: request.label, excerpt: request.excerpt, ref: request.ref, path: "", content: request.content });
        callbacks.current.onDocked();
        return;
      }
      const ticket = generation.current + 1;
      generation.current = ticket;
      void storeBlob(new Blob([request.content], { type: "text/plain" }), contextFileName({ kind: "quote", detail: request.label }))
        .then((path) => {
          if (generation.current !== ticket || !path.trim()) return;
          set({ label: request.label, excerpt: request.excerpt, ref: request.ref, path, content: request.content });
          callbacks.current.onDocked();
        })
        .catch((error: unknown) => callbacks.current.onError(error instanceof Error ? error.message : String(error)));
    };
    drain();
    return composerReplyStore.subscribe(drain);
  }, [set, tabId]);

  return { reply, replyRef, clear, clearIfSent };
}
