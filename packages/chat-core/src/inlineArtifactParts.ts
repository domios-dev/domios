import type { AttachmentSegment } from "./attachmentMarker";
import { localArtifactTextParts, type LocalArtifactDirective } from "./localArtifactDirective";

export type InlineArtifactPart = AttachmentSegment | { directive: LocalArtifactDirective };

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
  const result: InlineArtifactPart[] = [];
  let offset = 0;
  for (const part of localArtifactTextParts(source)) {
    const end = offset + part.text.length;
    const capsules = [...attachments].filter(([at]) => at >= offset && at < end);
    if (part.directive && capsules.length === 0) result.push({ directive: part.directive });
    else {
      let cursor = offset;
      for (const [at, capsule] of capsules) {
        if (at > cursor) result.push({ text: source.slice(cursor, at) });
        result.push(capsule);
        cursor = at + 1;
      }
      if (cursor < end) result.push({ text: source.slice(cursor, end) });
    }
    offset = end;
  }
  return result;
}
