import type { ReactNode } from "react";
import { placedAttachmentSegments, type ChatAttachment } from "@codeterm/chat-core";
import { InlineAttachmentText } from "./AttachmentCapsule";

export function OrderedAttachmentContent({ content, attachments = [], renderText }: {
  content: string;
  attachments?: ChatAttachment[];
  renderText: (text: string) => ReactNode;
}) {
  const segments = placedAttachmentSegments(content, attachments);
  return segments.some((segment) => "attachment" in segment)
    ? <InlineAttachmentText segments={segments} />
    : <>{renderText(content)}</>;
}
