import { attachmentMarkerSegments, parseAttachmentMarkers, type AttachmentMarker } from "./attachmentMarker";
import { normalizeChatText } from "./normalize";
import { userEchoBody } from "./userEchoBody";

function pathKey(path: string): string {
  return path.replace(/\\/g, "/");
}

function sameAttachment(left: AttachmentMarker, right: AttachmentMarker): boolean {
  return left.kind === right.kind && pathKey(left.path) === pathKey(right.path)
    && left.label === right.label && left.reply === right.reply && left.excerpt === right.excerpt;
}

const UNAVAILABLE_NOTE = " [Model preview unavailable: This host cannot create a model preview for this image format; use the original file path or attach a JPEG or PNG. Original kept.]";
const SYSTEM_IMAGE = /\.(heic|heif|avif|tiff?|bmp|dng|raw|cr2|cr3|nef|arw|orf|rw2|raf|pef|srw|gif|webp)$/i;

function withoutUnavailableNotes(body: string, originals: AttachmentMarker[]) {
  const paths = new Set<string>();
  const text = body.replaceAll(UNAVAILABLE_NOTE, (note, offset: number) => {
    const segments = attachmentMarkerSegments(body.slice(0, offset));
    const previous = segments[segments.length - 1];
    if (!previous || !("attachment" in previous)) return note;
    const file = previous.attachment;
    if (file.kind !== "file" || !SYSTEM_IMAGE.test(file.path)
      || !originals.some(original => (original.kind === "image" || original.kind === "file")
        && sameAttachment({ ...original, kind: "file" }, file))) return note;
    paths.add(pathKey(file.path));
    return "";
  });
  return { text, paths };
}

export function modelImageEchoMatches(original: string, delivered: string): boolean {
  if (!delivered.includes("-model.jpg") && !delivered.includes(UNAVAILABLE_NOTE)) return false;
  const before = parseAttachmentMarkers(userEchoBody(original));
  const unavailable = withoutUnavailableNotes(userEchoBody(delivered), before.attachments);
  const after = parseAttachmentMarkers(unavailable.text);
  if (!before.attachments.length || after.attachments.length < before.attachments.length
    || normalizeChatText(before.text) !== normalizeChatText(after.text)) return false;
  let index = 0;
  let converted = false;
  for (const attachment of before.attachments) {
    const current = after.attachments[index];
    if (!current) return false;
    const copy = after.attachments[index + 1];
    if ((attachment.kind === "image" || attachment.kind === "file") && !attachment.reply && !attachment.excerpt
      && current.kind === "file" && current.label === "original" && !current.reply && !current.excerpt
      && pathKey(current.path) === pathKey(attachment.path) && copy?.kind === "image"
      && copy.label === attachment.label && !copy.reply && !copy.excerpt && /-model\.jpg$/i.test(copy.path)
      && !before.attachments.some(item => pathKey(item.path) === pathKey(copy.path))) {
      index += 2;
      converted = true;
    } else if (unavailable.paths.has(pathKey(attachment.path)) && (attachment.kind === "image" || attachment.kind === "file")
      && sameAttachment({ ...attachment, kind: "file" }, current)) {
      index++;
      converted = true;
    } else if (sameAttachment(attachment, current)) index++;
    else return false;
  }
  return converted && index === after.attachments.length;
}
