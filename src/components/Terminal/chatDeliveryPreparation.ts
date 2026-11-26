import { hasAttachmentMarkers } from "@codeterm/chat-core";
import type { DeliverySend } from "./chatDeliveryModel";
import { composerAttachmentHandoff } from "./composerAttachmentHandoff";
import { attachmentHostTarget, composeAttachmentDraft } from "./meshAttachmentHandoff";
import { failChatSendRequest, setChatSendDraft, updateChatSendContent } from "./chatSendStore";
import type { DsTasksExpansion } from "./dsTasks/dsTasksModel";

export async function prepareChatDelivery(tabId: string, send: DeliverySend, expand: (text: string) => DsTasksExpansion | Promise<DsTasksExpansion>): Promise<string> {
  const draft = send.draft;
  if (!draft) {
    if (attachmentHostTarget(tabId, composerAttachmentHandoff.relayPeerId()).kind === "remote" && hasAttachmentMarkers(send.content)) {
      try {
        const result = await composeAttachmentDraft(tabId, send.content, [], [], composerAttachmentHandoff);
        updateChatSendContent(tabId, send.id, result.composed);
        return result.composed;
      } catch (error) {
        failChatSendRequest(tabId, send.id, error);
        throw error;
      }
    }
    return send.content;
  }
  try {
    const result = await composeAttachmentDraft(tabId, draft.text, draft.attachments,
      draft.recordedVoiceAttachments, composerAttachmentHandoff, draft.capsules, draft.reply, draft.inlineReply);
    if (result.transcribeFailed) throw new Error("Voice transcription failed");
    const expansion = await expand(result.composed);
    if (expansion.kind === "error") throw new Error(expansion.message);
    const composed = expansion.kind === "expanded" ? expansion.text : result.composed;
    if (!composed.trim()) throw new Error("No message was sent because the prepared message is empty.");
    updateChatSendContent(tabId, send.id, composed);
    setChatSendDraft(tabId, send.id, undefined);
    return composed;
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    if (!draft.recordedVoiceAttachments.length) updateChatSendContent(tabId, send.id, `Message was not sent: ${reason}`);
    failChatSendRequest(tabId, send.id, error);
    throw error;
  }
}
