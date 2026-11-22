import { attachmentKindForPath, parseAttachmentMarkers, type AttachmentKind } from "@codeterm/chat-core";
import type { ProviderAttachmentCapabilities } from "../../lib/types";
import { capsulesInDraft, type CapsuleRegistry } from "./composerCapsules";
import type { ComposerReply } from "./composerReply";

const AUDIO_PATH = /\.(?:mp3|wav|ogg|oga|m4a|flac)$/i;

export function composerAttachmentError(
  capabilities: (ProviderAttachmentCapabilities & { audioPaths?: boolean }) | undefined,
  text: string,
  paths: readonly string[],
  recordedVoice: readonly string[],
  registry: CapsuleRegistry,
  reply: ComposerReply | null = null,
): string | null {
  const transcribed = new Set(recordedVoice);
  const kinds: Array<AttachmentKind | "audio"> = [
    ...paths
      .filter(path => !transcribed.has(path))
      .map(path => (AUDIO_PATH.test(path) ? "audio" as const : attachmentKindForPath(path))),
    ...capsulesInDraft(text, registry).map(capsule => capsule.kind),
    ...parseAttachmentMarkers(text).attachments.map(attachment => attachment.kind),
    ...(reply ? ["quote" as const] : []),
  ];
  if (!kinds.length) return null;
  if (!capabilities) return "Attachment support is unavailable for this harness. Remove attachments before sending.";
  for (const kind of kinds) {
    const allowed = kind === "image"
      ? capabilities.imagePaths
      : kind === "audio" || kind === "voice"
        ? capabilities.audioPaths === true || capabilities.filePaths
        : kind === "quote"
          ? capabilities.filePaths || capabilities.inlineText
          : capabilities.filePaths;
    if (allowed) continue;
    const suggestion = capabilities.inlineText ? " Paste text directly into the message instead." : "";
    const label = kind === "image" ? "image" : kind === "audio" || kind === "voice" ? "audio" : "file";
    return `This harness cannot read ${label} attachments. Remove the attachment before sending.${suggestion}`;
  }
  return null;
}
