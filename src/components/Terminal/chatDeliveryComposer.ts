import { useEffect, useRef } from "react";
import { formatAttachmentMarker } from "@codeterm/chat-core";
import { serializeCapsuleDraft } from "./composerCapsules";
import { replyMarker, withReplyMarker } from "./composerReply";
import { canEditChatSend, type DeliverySend } from "./chatDeliveryModel";

const pending = new Map<string, string[]>();
const listeners = new Set<() => void>();

export function editableChatDelivery(send: DeliverySend): string {
  const draft = send.draft;
  if (!draft) return send.content;
  const body = [serializeCapsuleDraft(draft.text, draft.capsules),
    ...draft.attachments.map(path => formatAttachmentMarker(path)),
    ...draft.recordedVoiceAttachments.map(path => formatAttachmentMarker(path, "voice")),
  ].filter(Boolean).join("\n");
  return draft.reply ? withReplyMarker(replyMarker(draft.reply), body) : body;
}

export function editChatDelivery(tabId: string, send: DeliverySend): void {
  if (!canEditChatSend(send)) return;
  pending.set(tabId, [...(pending.get(tabId) ?? []), editableChatDelivery(send)]);
  for (const listener of listeners) listener();
}

export function useChatDeliveryComposer(tabId: string, restore: (text: string) => void): void {
  const restoreRef = useRef(restore);
  restoreRef.current = restore;
  useEffect(() => {
    const drain = () => {
      const texts = pending.get(tabId);
      if (!texts) return;
      pending.delete(tabId);
      restoreRef.current(texts.join("\n"));
    };
    listeners.add(drain);
    drain();
    return () => { listeners.delete(drain); };
  }, [tabId]);
}
