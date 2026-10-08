import { imageMimeType, parseAttachmentMarkers, type AttachmentMarker } from "./attachmentMarker";
import type { ChatAttachment } from "./types";

const IMAGE_COUNT_LABEL = /^\[\d+ images?\]$/;

/** Text a user bubble shows; the host's `[N image(s)]` stand-in is redundant beside the attachments. */
export function userVisibleText(content: string, attachments?: ChatAttachment[]): string {
  if (attachments?.length && IMAGE_COUNT_LABEL.test(content.trim())) return "";
  return content;
}

export interface UserMessageAttachments {
  text: string;
  images: ChatAttachment[];
  files: AttachmentMarker[];
}

function pathKey(path: string): string {
  return path.replace(/\\/g, "/");
}

/** What a user bubble renders: clean text, one image strip from provider-native and marker images, and file chips. */
export function userMessageAttachments(content: string, attachments: ChatAttachment[] = []): UserMessageAttachments {
  const parsed = parseAttachmentMarkers(content);
  const images = [...attachments];
  const seen = new Set(attachments.flatMap((attachment) => (attachment.path ? [pathKey(attachment.path)] : [])));
  const files: AttachmentMarker[] = [];
  for (const marker of parsed.attachments) {
    if (seen.has(pathKey(marker.path))) continue;
    seen.add(pathKey(marker.path));
    if (marker.kind === "file") files.push(marker);
    else images.push({ id: `attachment:${marker.path}`, mimeType: imageMimeType(marker.path), path: marker.path });
  }
  return { text: userVisibleText(parsed.text, images), images, files };
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
