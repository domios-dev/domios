import { attachmentMarkerSegments, imageMimeType, parseAttachmentMarkers, splitReplyAttachment, type AttachmentMarker, type AttachmentSegment } from "./attachmentMarker";
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
  /** Text with capsules where they were typed; null when the message has no inline capsule. */
  inline: AttachmentSegment[] | null;
  /** The quote this message replies to, rendered as a block at the top of the bubble. */
  reply: AttachmentMarker | null;
}

function pathKey(path: string): string {
  return path.replace(/\\/g, "/");
}

function inlineSegments(content: string): AttachmentSegment[] | null {
  const segments = attachmentMarkerSegments(content).filter((segment) => !("attachment" in segment) || segment.attachment.label);
  if (!segments.some((segment) => "attachment" in segment)) return null;
  const first = segments[0];
  const last = segments[segments.length - 1];
  if ("text" in first) first.text = first.text.trimStart();
  if ("text" in last) last.text = last.text.trimEnd();
  return segments.filter((segment) => !("text" in segment) || segment.text);
}

/** What a user bubble renders: clean text, inline capsules, one image strip from provider-native and marker images, and file chips. */
export function userMessageAttachments(source: string, attachments: ChatAttachment[] = []): UserMessageAttachments {
  const { reply, text: content } = splitReplyAttachment(source);
  const parsed = parseAttachmentMarkers(content);
  const images = [...attachments];
  const seen = new Set(attachments.flatMap((attachment) => (attachment.path ? [pathKey(attachment.path)] : [])));
  const files: AttachmentMarker[] = [];
  for (const marker of parsed.attachments) {
    if (marker.label || seen.has(pathKey(marker.path))) continue;
    seen.add(pathKey(marker.path));
    if (marker.kind !== "image") files.push(marker);
    else images.push({ id: `attachment:${marker.path}`, mimeType: imageMimeType(marker.path), path: marker.path });
  }
  return { text: userVisibleText(parsed.text, images), images, files, inline: inlineSegments(content), reply };
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

export type PlacedAttachmentSegment = { text: string } | { attachment: AttachmentMarker; image?: ChatAttachment };

export function placedAttachmentSegments(source: string, attachments: ChatAttachment[] = []): PlacedAttachmentSegment[] {
  const content = userVisibleText(splitReplyAttachment(source).text, attachments);
  const segments = attachmentMarkerSegments(content);
  const paths = new Set(segments.flatMap((segment) => "attachment" in segment ? [pathKey(segment.attachment.path)] : []));
  const placed: PlacedAttachmentSegment[] = segments.map((segment) => "text" in segment ? segment : {
    attachment: segment.attachment,
    image: attachments.find((image) => image.path && pathKey(image.path) === pathKey(segment.attachment.path)),
  });
  for (const image of attachments) {
    if (image.path && paths.has(pathKey(image.path))) continue;
    if (placed.length) placed.push({ text: " " });
    placed.push({ attachment: { kind: "image", path: image.path ?? "", label: attachmentFileName(image) }, image });
    if (image.path) paths.add(pathKey(image.path));
  }
  return placed;
}
