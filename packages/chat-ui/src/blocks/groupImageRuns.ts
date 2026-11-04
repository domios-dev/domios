import type { DsSegment } from "@codeterm/chat-core";
import { GALLERY_PATH_LIMIT } from "../galleryLoading";

type ImageSegment = Extract<DsSegment, { type: "block" }> & { block: { kind: "image"; sources: string[] } };
const isImage = (segment: DsSegment | undefined): segment is ImageSegment => segment?.type === "block" && segment.block.kind === "image";

/** Image blocks separated only by blank lines become one gallery, up to the gallery's path limit. */
export function groupImageRuns(segments: readonly DsSegment[]): readonly DsSegment[] {
  if (segments.filter(isImage).length < 2) return segments;
  const grouped: DsSegment[] = [];
  let gap: DsSegment[] = [];
  for (const segment of segments) {
    const last = grouped[grouped.length - 1];
    if (isImage(segment) && isImage(last) && last.block.sources.length + segment.block.sources.length <= GALLERY_PATH_LIMIT) {
      grouped[grouped.length - 1] = { ...last, end: segment.end, block: { kind: "image", sources: [...last.block.sources, ...segment.block.sources] } };
      gap = [];
      continue;
    }
    if (segment.type === "markdown" && !segment.text.trim() && isImage(last)) { gap.push(segment); continue; }
    grouped.push(...gap, segment);
    gap = [];
  }
  grouped.push(...gap);
  return grouped;
}
