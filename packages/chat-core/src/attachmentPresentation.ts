import { attachmentPathName, type PlacedAttachmentSegment } from "./userAttachments";

function hashed(name: string): boolean {
  return /^[a-f\d]{16,}/i.test(name);
}

function originalName(path: string): string {
  const name = attachmentPathName(path);
  return hashed(name) ? `Image.${name.split(".").pop() || "jpg"}` : name;
}

export function presentedAttachmentSegments(segments: readonly PlacedAttachmentSegment[], names: Readonly<Record<string, string>> = {}): PlacedAttachmentSegment[] {
  const result: PlacedAttachmentSegment[] = [];
  for (let index = 0; index < segments.length; index++) {
    const segment = segments[index];
    if ("text" in segment) { result.push(segment); continue; }
    const original = segment.attachment;
    let next = index + 1;
    while (next < segments.length && "text" in segments[next] && !(segments[next] as { text: string }).text.trim()) next++;
    const copy = segments[next];
    if (original.kind === "file" && original.label === "original" && copy && "attachment" in copy
      && copy.attachment.kind === "image" && /-model\.jpg$/i.test(copy.attachment.path)) {
      result.push({ attachment: { ...original, kind: "image", label: names[original.path] ?? originalName(original.path), previewPath: copy.attachment.path }, image: copy.image });
      index = next;
    } else {
      const label = names[original.path] ?? (original.kind === "image" || hashed(original.label ?? "") ? originalName(original.path) : original.label);
      result.push({ ...segment, attachment: { ...original, label } });
    }
  }
  return result.map((segment, index) => "text" in segment && !segment.text.trim()
    && result[index - 1] && "attachment" in result[index - 1] && result[index + 1] && "attachment" in result[index + 1]
    ? { text: " " } : segment);
}
