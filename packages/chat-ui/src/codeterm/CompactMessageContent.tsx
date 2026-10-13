import React from "react";
import { userMessageAttachments } from "@codeterm/chat-core";
import { LinkedText, type OpenPlan, type ResolvedChatPlanRef } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";
import { RichMarkdown } from "../MarkdownBody";
import { normalizeMessageText } from "./messageText";
import { pickUiFontScaleValue } from "../uiFontScale";
import { AttachmentStrip } from "../AttachmentStrip";
import { OrderedAttachmentContent } from "../OrderedAttachmentContent";
import { InlineAttachmentText } from "../AttachmentCapsule";
import { ReplyQuote } from "../ReplyQuote";

interface Props {
  text: string;
  fontScale?: number;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  collapseAfter?: number;
  expandTestId?: string;
}

/** Body of every inbound and system notice: markdown text plus the same quote, image and file rendering a user bubble gets. */
export function CompactMessageContent({ text, fontScale, onOpenPlan, resolvePlanRef, collapseAfter = 8, expandTestId }: Props) {
  const parts = React.useMemo(() => userMessageAttachments(text), [text]);
  const normalized = normalizeMessageText(parts.text);
  const lines = normalized.split("\n");
  const collapsible = lines.length > collapseAfter;
  const [expanded, setExpanded] = React.useState(false);
  const textSize = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const renderText = (content: string) => {
    const part = userMessageAttachments(content);
    const normalized = normalizeMessageText(part.text);
    const shown = expanded || !collapsible ? normalized : normalized.split("\n").slice(0, collapseAfter).join("\n");
    const body = part.inline ? <InlineAttachmentText segments={part.inline} />
      : hasPlanRefs(shown) ? <LinkedText text={shown} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
      : <RichMarkdown text={shown} fontScale={fontScale} />;
    return <>{part.files.length > 0 && <AttachmentStrip images={[]} files={part.files} captioned={Boolean(shown.trim())} />}{body}</>;
  };

  return (
    <div data-testid="codeterm-message-content" className={`min-w-0 ${textSize}`}>
      {parts.reply && <ReplyQuote label={parts.reply.label ?? "Quote"} excerpt={parts.reply.excerpt ?? ""} />}
      <OrderedAttachmentContent content={text} renderText={renderText} />
      {collapsible && !parts.inline && (
        <button
          type="button"
          data-testid={expandTestId}
          onClick={() => setExpanded((value) => !value)}
          className="mt-1 text-[11px] underline"
          style={{ color: "var(--ct-accent)" }}
        >
          {expanded ? "Show less" : `Show ${lines.length - collapseAfter} more lines`}
        </button>
      )}
    </div>
  );
}
