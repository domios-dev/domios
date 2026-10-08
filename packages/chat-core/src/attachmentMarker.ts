import type { LocalArtifactKind } from "./localArtifactDirective";

export type AttachmentKind = "image" | "file";

export interface AttachmentMarker {
  kind: AttachmentKind;
  path: string;
}

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
const DOCUMENT_EXTENSIONS = new Set(["md", "markdown", "txt", "log", "json", "yaml", "yml", "toml", "csv"]);

const MARKER = /<ds-attachment\b([^<>]*?)\/?>/gi;
const ATTRIBUTE = /([a-z]+)="([^"]*)"/gi;
const LEGACY_DIR = String.raw`(?:codeterm-chat|\.codeterm[\w-]*[\\/]uploads)`;
const LEGACY_ROOT = String.raw`(?:[A-Za-z]:[\\/]|/)`;
const LEGACY_FILE = String.raw`[^\s"'<>|:\\/]+\.[A-Za-z0-9]+`;
// A host temp attachment path pasted bare by older composers; quoted when the temp dir has spaces.
const LEGACY_PATH = new RegExp(
  String.raw`(?<=^|\s)(?:'(${LEGACY_ROOT}[^'\n]*?${LEGACY_DIR}[\\/]${LEGACY_FILE})'|"(${LEGACY_ROOT}[^"\n]*?${LEGACY_DIR}[\\/]${LEGACY_FILE})"|(${LEGACY_ROOT}(?:[^\s"'<>|]*?[\\/])?${LEGACY_DIR}[\\/]${LEGACY_FILE}))(?=\s|$)`,
  "g",
);

function extension(path: string): string {
  return /\.([a-z0-9]+)$/i.exec(path)?.[1]?.toLowerCase() ?? "";
}

export function isImagePath(path: string): boolean {
  return extension(path) in IMAGE_MIME;
}

export function imageMimeType(path: string): string {
  return IMAGE_MIME[extension(path)] ?? "image/png";
}

export function attachmentKindForPath(path: string): AttachmentKind {
  return isImagePath(path) ? "image" : "file";
}

/** The local preview kind a Documents list can open, or null when the file has no preview. */
export function attachmentPreviewKind(attachment: AttachmentMarker): LocalArtifactKind | null {
  const ext = extension(attachment.path);
  if (attachment.kind === "image") return ext in IMAGE_MIME ? "image" : null;
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
export function formatAttachmentMarker(path: string, kind: AttachmentKind = attachmentKindForPath(path)): string {
  return `<ds-attachment kind="${kind}" path="${escapeAttribute(path)}" />`;
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
  const start = match.index ?? 0;
  return {
    start,
    end: start + match[0].length,
    attachment: path ? { kind: kind === "image" || kind === "file" ? kind : attachmentKindForPath(path), path } : null,
  };
}

function legacySpan(match: RegExpMatchArray): Span {
  const path = match[1] ?? match[2] ?? match[3];
  const start = match.index ?? 0;
  return { start, end: start + match[0].length, attachment: { kind: attachmentKindForPath(path), path } };
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

/** Splits attachment markers, and bare host temp attachment paths from older transcripts, out of message text. */
export function parseAttachmentMarkers(content: string): ParsedAttachmentMarkers {
  const markers = [...content.matchAll(MARKER)].map(markerSpan);
  const legacy = [...content.matchAll(LEGACY_PATH)]
    .map(legacySpan)
    .filter((span) => !markers.some((marker) => span.start < marker.end && marker.start < span.end));
  const spans = [...markers, ...legacy].sort((a, b) => a.start - b.start);
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
