import { parseAttachmentMarkers, type AttachmentMarker } from "./attachmentMarker";
import { normalizeChatText } from "./normalize";
import { userEchoBody } from "./userEchoBody";

function pathKey(path: string): string {
  return path.replace(/\\/g, "/");
}

function sameAttachment(left: AttachmentMarker, right: AttachmentMarker): boolean {
  return left.kind === right.kind && pathKey(left.path) === pathKey(right.path)
    && left.label === right.label && left.reply === right.reply && left.excerpt === right.excerpt;
}

export function modelImageEchoMatches(original: string, delivered: string): boolean {
  if (!delivered.includes('label="original"') || !delivered.includes("-model.jpg")) return false;
  const before = parseAttachmentMarkers(userEchoBody(original));
  const after = parseAttachmentMarkers(userEchoBody(delivered));
  if (!before.attachments.length || after.attachments.length <= before.attachments.length
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
    } else if (sameAttachment(attachment, current)) index++;
    else return false;
  }
  return converted && index === after.attachments.length;
}
