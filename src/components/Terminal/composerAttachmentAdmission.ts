import { attachmentKindForPath, parseAttachmentMarkers, type AttachmentKind } from "@codeterm/chat-core";
import type { ProviderAttachmentCapabilities } from "../../lib/types";
import { capsulesInDraft, type CapsuleRegistry } from "./composerCapsules";
import type { ComposerReply } from "./composerReply";

export function composerAttachmentError(
  capabilities: ProviderAttachmentCapabilities | undefined,
  text: string,
  paths: readonly string[],
  recordedVoice: readonly string[],
  registry: CapsuleRegistry,
  reply: ComposerReply | null = null,
): string | null {
  const transcribed = new Set(recordedVoice);
  const kinds: AttachmentKind[] = [
    ...paths.filter(path => !transcribed.has(path)).map(attachmentKindForPath),
    ...capsulesInDraft(text, registry).map(capsule => capsule.kind),
    ...parseAttachmentMarkers(text).attachments.map(attachment => attachment.kind),
    ...(reply ? ["quote" as const] : []),
  ];
  if (!kinds.length) return null;
  if (!capabilities) return "Attachment support is unavailable for this provider. Remove attachments before sending.";
  for (const kind of kinds) {
    if (kind === "image" ? capabilities.imagePaths : capabilities.filePaths) continue;
    const suggestion = capabilities.inlineText ? " Paste text directly into the message instead." : "";
    return `This provider cannot read ${kind === "image" ? "image" : "file"} attachments. Remove the attachment before sending.${suggestion}`;
  }
  return null;
}
