import React from "react";
import { userMessageAttachments } from "@codeterm/chat-core";
import { LinkedText, type OpenPlan, type ResolvedChatPlanRef } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";
import { RichMarkdown } from "../MarkdownBody";
import { normalizeMessageText } from "./messageText";
import { pickUiFontScaleValue } from "../uiFontScale";
import { AttachmentStrip } from "../AttachmentStrip";
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
  const shown = expanded || !collapsible ? normalized : lines.slice(0, collapseAfter).join("\n");
  const textSize = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const hasStrip = parts.images.length > 0 || parts.files.length > 0;

  return (
    <div data-testid="codeterm-message-content" className={`min-w-0 ${textSize}`}>
      {parts.reply && <ReplyQuote label={parts.reply.label ?? "Quote"} excerpt={parts.reply.excerpt ?? ""} />}
      {hasStrip && <AttachmentStrip images={parts.images} files={parts.files} captioned={normalized.trim().length > 0} />}
      {parts.inline ? (
        <InlineAttachmentText segments={parts.inline} />
      ) : hasPlanRefs(shown) ? (
        <LinkedText text={shown} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
      ) : (
        <RichMarkdown text={shown} fontScale={fontScale} />
      )}
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
