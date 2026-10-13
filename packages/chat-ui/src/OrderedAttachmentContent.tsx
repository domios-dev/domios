import type { ReactNode } from "react";
import { formatAttachmentMarker, imageMimeType, parseMessageParts, type AttachmentMarker, type ChatAttachment } from "@codeterm/chat-core";
import { AttachmentStrip } from "./AttachmentStrip";

export function OrderedAttachmentContent({ content, attachments = [], renderText }: {
  content: string;
  attachments?: ChatAttachment[];
  renderText: (text: string) => ReactNode;
}) {
  const parts = parseMessageParts(content);
  const paths = new Set(parts.flatMap((part) => part.kind === "attachment" ? part.attachments.map((marker) => marker.path.replace(/\\/g, "/")) : []));
  const remaining = attachments.filter((image) => !image.path || !paths.has(image.path.replace(/\\/g, "/")));
  const rows: Array<{ text: string } | { images: ChatAttachment[]; files: AttachmentMarker[] }> = [];
  for (const part of parts) {
    const images = part.kind === "attachment" ? part.attachments.filter((marker) => marker.kind === "image") : [];
    const files = part.kind === "attachment" ? part.attachments.filter((marker) => marker.kind !== "image" && !marker.label) : [];
    if (images.length) rows.push({ files, images: images.map((marker) =>
      attachments.find((image) => image.path?.replace(/\\/g, "/") === marker.path.replace(/\\/g, "/"))
        ?? { id: `attachment:${marker.path}`, mimeType: imageMimeType(marker.path), path: marker.path }) });
    const text = part.kind === "attachment" ? part.attachments.filter((marker) => marker.kind !== "image" && (!images.length || marker.label))
      .map((marker) => formatAttachmentMarker(marker.path, marker.kind, marker.label)).join(" ") : part.content;
    if (text) {
      const previous = rows[rows.length - 1];
      if (previous && "text" in previous) previous.text += text;
      else rows.push({ text });
    }
  }
  return <>
    {remaining.length > 0 && <AttachmentStrip images={remaining} files={[]} captioned={Boolean(content.trim())} />}
    {rows.map((row, index) => <div key={index}>{"text" in row ? renderText(row.text)
      : <AttachmentStrip images={row.images} files={row.files} captioned={index < rows.length - 1} />}</div>)}
  </>;
}
