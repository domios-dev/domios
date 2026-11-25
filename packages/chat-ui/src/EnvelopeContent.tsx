import { ATTACHMENT_KINDS, type AttachmentKind, type ChatAttachment, type MessageSegment, type PlacedAttachmentSegment } from "@codeterm/chat-core";
import type { ReactNode } from "react";
import { InlineAttachmentText } from "./AttachmentCapsule";
import { VoiceCard } from "./MarkdownBody";
import { ReplyQuote } from "./ReplyQuote";

function segmentText(segment: Exclude<MessageSegment, { type: "frame" | "attachment" }>): string {
  if (segment.type === "text") return segment.text;
  if (segment.type === "unknown_frame" && !segment.body && !segment.attrs.from) return segment.raw;
  return segment.body;
}

export function EnvelopeContent({ segments, attachments = [], renderText, renderInlineText, onJumpToMessage, attachmentNames }: {
  attachmentNames?: Readonly<Record<string, string>>;
  segments: readonly MessageSegment[];
  attachments?: ChatAttachment[];
  renderText: (text: string) => ReactNode;
  renderInlineText?: (text: string) => ReactNode;
  onJumpToMessage?: (id: string) => void;
}) {
  const paths = new Set(segments.flatMap((segment) => segment.type === "attachment" ? [segment.path] : []));
  if (segments.every((segment) => segment.type !== "frame" && (segment.type !== "attachment" || (segment.kind !== "voice" && !segment.reference)))) {
    const placed: PlacedAttachmentSegment[] = segments.flatMap((segment): PlacedAttachmentSegment[] => {
      if (segment.type === "frame") return [];
      if (segment.type !== "attachment") return [{ text: segmentText(segment) }];
      const kind: AttachmentKind = (ATTACHMENT_KINDS as readonly string[]).includes(segment.kind) ? segment.kind as AttachmentKind : "file";
      return [{ attachment: { kind, path: segment.path, label: segment.label ?? undefined }, image: attachments.find((image) => image.path === segment.path) }];
    });
    for (const image of attachments) {
      if (image.path && paths.has(image.path)) continue;
      placed.push({ attachment: { kind: "image", path: image.path ?? "" }, image });
    }
    return placed.some((part) => "attachment" in part)
      ? <InlineAttachmentText segments={placed} renderText={renderInlineText} attachmentNames={attachmentNames} />
      : <>{renderText(placed.map((part) => "text" in part ? part.text : "").join(""))}</>;
  }
  const groups: MessageSegment[][] = [];
  const special = (segment: MessageSegment) => segment.type === "frame" || (segment.type === "attachment" && (segment.kind === "voice" || !!segment.reference));
  for (const segment of segments) {
    const last = groups[groups.length - 1];
    if (special(segment) || !last || special(last[0])) groups.push([segment]);
    else last.push(segment);
  }
  return <>
    {groups.map((group, index) => {
      const segment = group[0];
      if (!special(segment)) {
        const groupPaths = new Set(group.flatMap(part => part.type === "attachment" ? [part.path] : []));
        return <EnvelopeContent key={index} segments={group} attachments={attachments.filter(image => image.path && groupPaths.has(image.path))}
          renderText={renderText} renderInlineText={renderInlineText} attachmentNames={attachmentNames} />;
      }
      if (segment.type === "frame") return null;
      if (segment.type !== "attachment") return <span key={index}>{renderText(segmentText(segment))}</span>;
      const reference = segment.reference;
      if (reference) return <ReplyQuote key={index} label={segment.label ?? "Quote"} excerpt={segment.excerpt ?? ""}
        onOpen={onJumpToMessage ? () => onJumpToMessage(reference) : undefined} />;
      if (segment.kind === "voice") return <VoiceCard key={index} v={{
        transcript: segment.status === "untranscribed" ? null : segment.body,
        note: segment.status === "untranscribed" ? segment.body : null,
        fileUrl: segment.path, fileName: segment.path.split(/[\\/]/).pop() ?? null,
      }} />;
      const kind: AttachmentKind = (ATTACHMENT_KINDS as readonly string[]).includes(segment.kind) ? segment.kind as AttachmentKind : "file";
      return <InlineAttachmentText key={index} segments={[{
        attachment: { kind, path: segment.path, label: segment.label ?? undefined },
        image: attachments.find((attachment) => attachment.path === segment.path),
      }]} attachmentNames={attachmentNames} />;
    })}
    {attachments.filter((attachment) => !attachment.path || !paths.has(attachment.path)).map((image) =>
      <InlineAttachmentText key={image.id} segments={[{ attachment: { kind: "image", path: image.path ?? "" }, image }]} attachmentNames={attachmentNames} />)}
  </>;
}
