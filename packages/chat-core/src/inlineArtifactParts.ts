import type { AttachmentSegment } from "./attachmentMarker";
import { parseDsBlocks } from "./dsBlocks";
import { localArtifactFromBlock, type LocalArtifactDirective } from "./localArtifactDirective";

export type InlineArtifactPart = AttachmentSegment | { directive: LocalArtifactDirective };

export function inlineArtifactBlockBoundary(source: string, start: number, end: number, attachmentOffsets: readonly number[]): boolean {
  const lineStart = source.lastIndexOf("\n", start - 1) + 1;
  const newline = source.indexOf("\n", end);
  const lineEnd = newline < 0 ? source.length : newline;
  for (const offset of attachmentOffsets) if (offset >= lineStart && offset < lineEnd) return false;
  return true;
}

export function inlineArtifactParts(segments: AttachmentSegment[]): InlineArtifactPart[] {
  let source = "";
  const attachments = new Map<number, AttachmentSegment>();
  for (const segment of segments) {
    if ("text" in segment) source += segment.text;
    else {
      attachments.set(source.length, segment);
      source += "\0";
    }
  }
  const attachmentOffsets = [...attachments.keys()];
  const result: InlineArtifactPart[] = [];
  for (const part of parseDsBlocks(source)) {
    const offset = part.start;
    const end = part.end;
    const capsules = [...attachments].filter(([at]) => at >= offset && at < end);
    const directive = part.type === "block" && inlineArtifactBlockBoundary(source, offset, end, attachmentOffsets) ? localArtifactFromBlock(part.block) : null;
    if (directive && capsules.length === 0) result.push({ directive });
    else {
      let cursor = offset;
      for (const [at, capsule] of capsules) {
        if (at > cursor) result.push({ text: source.slice(cursor, at) });
        result.push(capsule);
        cursor = at + 1;
      }
      if (cursor < end) result.push({ text: source.slice(cursor, end) });
    }
  }
  return result;
}
