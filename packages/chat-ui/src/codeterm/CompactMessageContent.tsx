import React from "react";
import { LinkedText, type OpenPlan, type ResolvedChatPlanRef } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";
import { RichMarkdown } from "../MarkdownBody";
import { normalizeMessageText } from "./messageText";
import { pickUiFontScaleValue } from "../uiFontScale";

interface Props {
  text: string;
  fontScale?: number;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  collapseAfter?: number;
  expandTestId?: string;
}

export function CompactMessageContent({ text, fontScale, onOpenPlan, resolvePlanRef, collapseAfter = 8, expandTestId }: Props) {
  const normalized = normalizeMessageText(text);
  const lines = normalized.split("\n");
  const collapsible = lines.length > collapseAfter;
  const [expanded, setExpanded] = React.useState(false);
  const shown = expanded || !collapsible ? normalized : lines.slice(0, collapseAfter).join("\n");
  const textSize = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);

  return (
    <div data-testid="codeterm-message-content" className={`min-w-0 ${textSize}`}>
      {hasPlanRefs(shown) ? (
        <LinkedText text={shown} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
      ) : (
        <RichMarkdown text={shown} fontScale={fontScale} />
      )}
      {collapsible && (
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
