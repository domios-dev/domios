import type { DeliverySend } from "./chatDeliveryModel";
import { composerAttachmentHandoff } from "./composerAttachmentHandoff";
import { composeAttachmentDraft } from "./meshAttachmentHandoff";
import { failChatSendRequest, setChatSendDraft, updateChatSendContent } from "./chatSendStore";
import type { DsTasksExpansion } from "./dsTasks/dsTasksModel";

export async function prepareChatDelivery(tabId: string, send: DeliverySend, expand: (text: string) => DsTasksExpansion): Promise<string> {
  const draft = send.draft;
  if (!draft) return send.content;
  try {
    const result = await composeAttachmentDraft(tabId, draft.text, draft.attachments,
      draft.recordedVoiceAttachments, composerAttachmentHandoff, draft.capsules, draft.reply);
    if (result.transcribeFailed) throw new Error("Voice transcription failed");
    const expansion = expand(result.composed);
    if (expansion.kind === "error") throw new Error(expansion.message);
    const composed = expansion.kind === "expanded" ? expansion.text : result.composed;
    updateChatSendContent(tabId, send.id, composed);
    setChatSendDraft(tabId, send.id, undefined);
    return composed;
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    updateChatSendContent(tabId, send.id, `Message was not sent: ${reason}`);
    failChatSendRequest(tabId, send.id);
    throw error;
  }
}
