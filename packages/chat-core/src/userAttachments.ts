import type { ChatAttachment } from "./types";

const IMAGE_COUNT_LABEL = /^\[\d+ images?\]$/;

/** Text a user bubble shows; the host's `[N image(s)]` stand-in is redundant beside the attachments. */
export function userVisibleText(content: string, attachments?: ChatAttachment[]): string {
  if (attachments?.length && IMAGE_COUNT_LABEL.test(content.trim())) return "";
  return content;
}

export function attachmentFileName(attachment: ChatAttachment): string {
  const source = attachment.path || attachment.url || "";
  const last = source.split(/[\\/]/).filter(Boolean).pop()?.split(/[?#]/)[0];
  if (last && !source.startsWith("data:")) return last;
  return `image.${attachment.mimeType.replace(/^image\//, "").split("+")[0] || "png"}`;
}

/** A renderable `src` available without IO; local paths need the host loader. */
export function inlineAttachmentSrc(attachment: ChatAttachment): string | null {
  if (attachment.data) return `data:${attachment.mimeType};base64,${attachment.data}`;
  if (attachment.url?.startsWith("data:image/")) return attachment.url;
  return null;
}
