import { AttachedDiscussionDocView, parseAttachedDiscussionDoc, RichMarkdown } from "@codeterm/chat-ui";
import React from "react";

export interface MilestoneTextProps {
  text: string;
  testId?: string;
}

export function MilestoneText({ text, testId = "canvas-milestone-plain-text" }: MilestoneTextProps) {
  const trimmed = text.trim();
  if (!trimmed) return null;
  const doc = parseAttachedDiscussionDoc(trimmed);
  if (doc) return <AttachedDiscussionDocView doc={doc} variant="canvas" />;
  return (
    React.createElement(
      "div",
      { "data-testid": testId, className: "ct-canvas-text", style: { overflowWrap: "anywhere" } },
      <RichMarkdown text={trimmed} />,
    )
  );
}
