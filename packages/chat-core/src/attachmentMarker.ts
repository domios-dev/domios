import type { LocalArtifactKind } from "./localArtifactDirective";

export const ATTACHMENT_KINDS = ["voice", "image", "video", "file", "text", "folder", "quote", "message", "diff", "document"] as const;
/** Context a capsule carries as text the agent reads by path: quoted passages, chat messages, diffs. */
export const CONTEXT_TEXT_KINDS = ["text", "quote", "message", "diff"] as const;
/** Clipboard flavour carrying text with attachment tags, so a composer paste rebuilds the capsules. */
export const DOMIOS_TEXT_MIME = "application/x-domios-text";
export type AttachmentKind = (typeof ATTACHMENT_KINDS)[number];

function isAttachmentKind(value: string | undefined): value is AttachmentKind {
  return (ATTACHMENT_KINDS as readonly string[]).includes(value ?? "");
}

export interface AttachmentMarker {
  kind: AttachmentKind;
  path: string;
  /** Set on a capsule typed inline in the composer; the bubble renders it where it was typed. */
  label?: string;
  /** Set on a reply quote: the id of the quoted message, which the bubble's quote block jumps to. */
  reply?: string;
  /** One line of the quoted text, shown in the reply block. */
  excerpt?: string;
}

export interface ReplyAttributes {
  ref: string;
  excerpt: string;
}

export type AttachmentSegment = { text: string } | { attachment: AttachmentMarker };

export interface ParsedAttachmentMarkers {
  text: string;
  attachments: AttachmentMarker[];
}

const IMAGE_MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  avif: "image/avif",
  svg: "image/svg+xml",
};
const VIDEO_MIME: Record<string, string> = {
  mp4: "video/mp4",
  m4v: "video/mp4",
  mov: "video/quicktime",
  webm: "video/webm",
};
const DOCUMENT_EXTENSIONS = new Set(["md", "markdown", "txt", "log", "json", "yaml", "yml", "toml", "csv"]);

// Only the typed attachment form matches; a routed `<domios from=...>` frame is provenance, never an attachment.
const MARKER = /<domios\b(?=[^<>]*\stype="attachment")([^<>]*?)\/?>/gi;
const ATTRIBUTE = /([a-z]+)="([^"]*)"/gi;

function extension(path: string): string {
  return /\.([a-z0-9]+)$/i.exec(path)?.[1]?.toLowerCase() ?? "";
}

export function isImagePath(path: string): boolean {
  return extension(path) in IMAGE_MIME;
}

export function imageMimeType(path: string): string {
  return IMAGE_MIME[extension(path)] ?? "image/png";
}

export function isVideoPath(path: string): boolean {
  return extension(path) in VIDEO_MIME;
}

export function videoMimeType(path: string): string {
  return VIDEO_MIME[extension(path)] ?? "video/mp4";
}

export function attachmentKindForPath(path: string): AttachmentKind {
  if (isImagePath(path)) return "image";
  return isVideoPath(path) ? "video" : "file";
}

/** The local preview kind a Documents list can open, or null when the file has no preview. */
export function attachmentPreviewKind(attachment: AttachmentMarker): LocalArtifactKind | null {
  const ext = extension(attachment.path);
  if (attachment.kind === "image") return ext in IMAGE_MIME ? "image" : null;
  if (attachment.kind === "video") return ext in VIDEO_MIME ? "video" : null;
  if (attachment.kind === "folder") return null;
  if ((CONTEXT_TEXT_KINDS as readonly string[]).includes(attachment.kind)) return "document";
  if (ext === "html" || ext === "htm") return "page";
  return DOCUMENT_EXTENSIONS.has(ext) ? "document" : null;
}

function escapeAttribute(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function unescapeAttribute(value: string): string {
  return value.replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
}

/** The text an agent receives for one attachment: the absolute path stays readable inside `path`. */
export function formatAttachmentMarker(path: string, kind: AttachmentKind = attachmentKindForPath(path), label?: string, reply?: ReplyAttributes): string {
  const labelAttribute = label ? ` label="${escapeAttribute(label)}"` : "";
  const replyAttributes = reply ? ` reply="${escapeAttribute(reply.ref)}" excerpt="${escapeAttribute(reply.excerpt)}"` : "";
  return `<domios type="attachment" kind="${kind}" path="${escapeAttribute(path)}"${labelAttribute}${replyAttributes} />`;
}

interface Span {
  start: number;
  end: number;
  attachment: AttachmentMarker | null;
}

function markerSpan(match: RegExpMatchArray): Span {
  const attributes = new Map<string, string>();
  for (const [, name, value] of match[1].matchAll(ATTRIBUTE)) attributes.set(name.toLowerCase(), unescapeAttribute(value));
  const path = attributes.get("path")?.trim() ?? "";
  const kind = attributes.get("kind");
  const label = attributes.get("label")?.trim();
  const start = match.index ?? 0;
  const attachment: AttachmentMarker | null = path ? { kind: isAttachmentKind(kind) ? kind : attachmentKindForPath(path), path } : null;
  if (attachment && label) attachment.label = label;
  const reply = attributes.get("reply");
  if (attachment && reply !== undefined) {
    attachment.reply = reply;
    attachment.excerpt = attributes.get("excerpt") ?? "";
  }
  return { start, end: start + match[0].length, attachment };
}

function removeSpans(content: string, spans: Span[]): string {
  let text = "";
  let cursor = 0;
  for (const span of spans) {
    const before = content.slice(cursor, span.start);
    const atLineStart = /(^|\n)[ \t]*$/.test(text + before);
    text += before.replace(/[ \t]+$/, "");
    cursor = span.end;
    if (atLineStart) cursor += /^[ \t]*/.exec(content.slice(cursor))?.[0].length ?? 0;
  }
  text += content.slice(cursor);
  return text.replace(/\n{3,}/g, "\n\n").trim();
}

function pathKey(path: string): string {
  return path.replace(/\\/g, "/");
}

export function hasAttachmentMarkers(content: string): boolean {
  return content.includes("<domios") && attachmentMarkerSegments(content).some((segment) => "attachment" in segment);
}

/** Splits attachment markers out of message text; text without markers is returned unchanged. */
export function parseAttachmentMarkers(content: string): ParsedAttachmentMarkers {
  const spans = [...content.matchAll(MARKER)].map(markerSpan);
  if (spans.length === 0) return { text: content, attachments: [] };
  const seen = new Set<string>();
  const attachments: AttachmentMarker[] = [];
  for (const { attachment } of spans) {
    if (!attachment || seen.has(pathKey(attachment.path))) continue;
    seen.add(pathKey(attachment.path));
    attachments.push(attachment);
  }
  return { text: removeSpans(content, spans), attachments };
}

/** Message text split at every marker, in order; a marker without a path stays out of both lists. */
export function attachmentMarkerSegments(content: string): AttachmentSegment[] {
  const segments: AttachmentSegment[] = [];
  let cursor = 0;
  for (const span of [...content.matchAll(MARKER)].map(markerSpan)) {
    if (span.start > cursor) segments.push({ text: content.slice(cursor, span.start) });
    if (span.attachment) segments.push({ attachment: span.attachment });
    cursor = span.end;
  }
  if (cursor < content.length) segments.push({ text: content.slice(cursor) });
  return segments;
}

/** The message's reply quote, taken out of its text; a message carries at most one. */
export function splitReplyAttachment(content: string): { reply: AttachmentMarker | null; text: string } {
  if (!content.includes("<domios")) return { reply: null, text: content };
  for (const match of content.matchAll(MARKER)) {
    const span = markerSpan(match);
    if (span.attachment?.reply === undefined) continue;
    const before = content.slice(0, span.start).trimEnd();
    const after = content.slice(span.end).replace(/^[ \t]*\r?\n?/, "");
    return { reply: span.attachment, text: before ? `${before}\n${after}` : after };
  }
  return { reply: null, text: content };
}
