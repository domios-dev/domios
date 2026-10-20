import { attachmentKindForPath, type AttachmentKind } from "./attachmentMarker";

export const MAX_VIDEO_BYTES = 200 * 1024 * 1024;

export type AttachmentVerdict = { ok: true } | { ok: false; error: string };

/** The kind an upload becomes: the browser's media type decides first, the extension second. */
export function attachmentKindForUpload(upload: { name: string; mime: string }): AttachmentKind {
  if (upload.mime.startsWith("image/")) return "image";
  if (upload.mime.startsWith("video/")) return "video";
  return attachmentKindForPath(upload.name);
}

export type VideoContainer = "mp4" | "mov" | "webm";

/** Container sniffed from leading bytes: ISO-BMFF `ftyp` (brand `qt  ` is QuickTime) or Matroska/WebM EBML. */
export function sniffVideoContainer(head: Uint8Array): VideoContainer | null {
  if (head.length >= 4 && head[0] === 0x1a && head[1] === 0x45 && head[2] === 0xdf && head[3] === 0xa3) return "webm";
  if (head.length < 12 || String.fromCharCode(...head.slice(4, 8)) !== "ftyp") return null;
  return String.fromCharCode(...head.slice(8, 12)) === "qt  " ? "mov" : "mp4";
}

export const VIDEO_SNIFF_BYTES = 16;

/** Accepts a video only when its bytes are a known container and it fits the upload cap; other kinds pass through. */
export function validateAttachment(input: { kind: AttachmentKind; size: number; head?: Uint8Array }): AttachmentVerdict {
  if (input.kind !== "video") return { ok: true };
  if (input.size > MAX_VIDEO_BYTES) {
    return { ok: false, error: `Video is ${Math.ceil(input.size / (1024 * 1024))} MB; the limit is ${MAX_VIDEO_BYTES / (1024 * 1024)} MB` };
  }
  if (input.head && !sniffVideoContainer(input.head)) return { ok: false, error: "Unsupported video format; use MP4, MOV or WebM" };
  return { ok: true };
}
