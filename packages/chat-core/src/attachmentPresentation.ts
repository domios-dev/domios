import { attachmentPathName, type PlacedAttachmentSegment } from "./userAttachments";

function hashed(name: string): boolean {
  return /^[a-f\d]{16,}/i.test(name);
}

function imageName(path: string): string {
  const name = attachmentPathName(path);
  return hashed(name) ? `Image.${name.split(".").pop() || "jpg"}` : name;
}

function storedFileLabel(path: string, label: string | undefined): string | undefined {
  if (!label) return label;
  const stored = path.split(/[\\/]/).pop() ?? "";
  if (!/^[a-f\d]{32}(?:[.-]|$)/i.test(stored) || !stored.startsWith(label.split("…")[0])) return label;
  const name = attachmentPathName(path);
  return /^[a-f\d]{32}\.txt$/i.test(name) ? "Pasted text.txt" : name;
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
      result.push({ attachment: { ...original, kind: "image", label: names[original.path] ?? imageName(original.path), previewPath: copy.attachment.path }, image: copy.image });
      index = next;
    } else {
      const label = names[original.path] ?? (original.kind === "image" ? imageName(original.path) : storedFileLabel(original.path, original.label));
      result.push({ ...segment, attachment: { ...original, label } });
    }
  }
  return result.map((segment, index) => "text" in segment && !segment.text.trim()
    && result[index - 1] && "attachment" in result[index - 1] && result[index + 1] && "attachment" in result[index + 1]
    ? { text: " " } : segment);
}
