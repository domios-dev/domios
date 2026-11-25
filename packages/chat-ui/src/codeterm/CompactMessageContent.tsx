import { ExpandableReader, useReaderHost } from "../ExpandableReader";
import React from "react";
import { useFullscreenViewer } from "../FullscreenOverlay";
import { useParentMessageBounds } from "../BoundedMessageContent";
import { hasTaskStrides, splitTaskStrides, userMessageAttachments } from "@codeterm/chat-core";
import type { MessageSegment } from "@codeterm/chat-core";
import { LinkedText, type OpenPlan, type ResolvedChatPlanRef } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";
import { RichMarkdown } from "../MarkdownBody";
import { TaskStridesBlock } from "../TaskStridesBlock";
import { normalizeMessageText } from "./messageText";
import { pickUiFontScaleValue } from "../uiFontScale";
import { AttachmentStrip } from "../AttachmentStrip";
import { OrderedAttachmentContent } from "../OrderedAttachmentContent";
import { InlineAttachmentText } from "../AttachmentCapsule";
import { ReplyQuote } from "../ReplyQuote";
import { EnvelopeContent } from "../EnvelopeContent";

interface Props {
  text: string;
  markdown?: boolean;
  segments?: MessageSegment[] | null;
  fontScale?: number;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  collapseAfter?: number;
  expandTestId?: string;
}

function visibleProseLineCount(normalized: string): number {
  if (!hasTaskStrides(normalized)) return normalized.split("\n").length;
  const parts = splitTaskStrides(normalized);
  if (!parts.some((part) => part.type === "strides")) return normalized.split("\n").length;
  let total = 0;
  for (const part of parts) {
    if (part.type === "text") total += part.text.split("\n").length;
  }
  return total;
}

/** Body of every inbound and system notice: markdown text plus the same quote, image and file rendering a user bubble gets. */
export function CompactMessageContent({ text, markdown = false, segments, fontScale, onOpenPlan, resolvePlanRef, collapseAfter = 8, expandTestId }: Props) {
  const parts = React.useMemo(() => segments ? { text, files: [], inline: null, reply: null } : userMessageAttachments(text), [text, segments]);
  const normalized = normalizeMessageText(parts.text);
  const proseLines = visibleProseLineCount(normalized);
  const readerHost = useReaderHost();
  const outerFullscreen = useFullscreenViewer();
  const parentOwnsBounds = useParentMessageBounds();
  const collapsible = !parentOwnsBounds && proseLines > collapseAfter;
  const [expanded, setExpanded] = React.useState(false);
  const textSize = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const renderProse = (shown: string, inlineParagraphs: boolean) =>
    !markdown && hasPlanRefs(shown)
      ? <LinkedText text={shown} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
      : <RichMarkdown text={shown} inlineParagraphs={inlineParagraphs} fontScale={fontScale} planLinks={{ onOpenPlan, resolvePlanRef }} />;
  const renderText = (content: string, fullscreen = false, inlineParagraphs = false) => {
    const part = segments ? { text: content, files: [], inline: null } : userMessageAttachments(content);
    const normalizedContent = normalizeMessageText(part.text);
    const showAll = fullscreen || outerFullscreen || expanded || !collapsible;
    if (!part.inline && hasTaskStrides(normalizedContent)) {
      const strideParts = splitTaskStrides(normalizedContent);
      if (strideParts.some((item) => item.type === "strides")) {
        let remaining = showAll ? Number.POSITIVE_INFINITY : collapseAfter;
        const nodes: React.ReactNode[] = [];
        for (let index = 0; index < strideParts.length; index += 1) {
          const item = strideParts[index];
          if (item.type === "strides") {
            nodes.push(
              <TaskStridesBlock
                key={index}
                block={item.block}
                renderBody={(body) => (
                  <RichMarkdown text={body} fontScale={fontScale} planLinks={{ onOpenPlan, resolvePlanRef }} />
                )}
              />,
            );
            continue;
          }
          if (remaining <= 0) continue;
          const itemLines = item.text.split("\n");
          const slice = itemLines.length <= remaining ? item.text : itemLines.slice(0, remaining).join("\n");
          remaining = Math.max(0, remaining - itemLines.length);
          nodes.push(<React.Fragment key={index}>{renderProse(slice, inlineParagraphs)}</React.Fragment>);
        }
        return (
          <>
            {part.files.length > 0 && <AttachmentStrip images={[]} files={part.files} captioned={Boolean(normalizedContent.trim())} />}
            {nodes}
          </>
        );
      }
    }
    const shown = showAll ? normalizedContent : normalizedContent.split("\n").slice(0, collapseAfter).join("\n");
    const body = part.inline ? <InlineAttachmentText segments={part.inline} /> : renderProse(shown, inlineParagraphs);
    return <>{part.files.length > 0 && <AttachmentStrip images={[]} files={part.files} captioned={Boolean(shown.trim())} />}{body}</>;
  };

  return (
    <ExpandableReader label={"Message content"} enabled={!readerHost && collapsible}>{(fullscreen) => (

    <div data-testid="codeterm-message-content" className={`min-w-0 ${textSize}`}>
      {parts.reply && <ReplyQuote label={parts.reply.label ?? "Quote"} excerpt={parts.reply.excerpt ?? ""} />}
      {segments ? <EnvelopeContent segments={segments} renderText={(text) => renderText(text, fullscreen)} renderInlineText={(text) => renderText(text, fullscreen, true)} /> : <OrderedAttachmentContent content={text} renderText={(text) => renderText(text, fullscreen)} renderInlineText={(text) => renderText(text, fullscreen, true)} />}
      {collapsible && !fullscreen && !outerFullscreen && !parts.inline && (
        <button
          type="button"
          data-testid={expandTestId}
          onClick={() => setExpanded((value) => !value)}
          className="mt-1 text-[11px] underline"
          style={{ color: "var(--ct-accent)" }}
        >
          {expanded ? "Show less" : `Show ${proseLines - collapseAfter} more lines`}
        </button>
      )}
    </div>
    )}</ExpandableReader>
  );
}
