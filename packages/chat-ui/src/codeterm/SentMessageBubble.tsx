import React from "react";
import type { CodetermInvocation } from "./recognize";
import { parseSend, type PaneNameResolver } from "./templates";
import { RichMarkdown } from "../MarkdownBody";
import { LinkedText, type OpenPlan, type ResolvedChatPlanRef } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";
import { pickUiFontScaleValue } from "../uiFontScale";
import { CrossPaneMessageShell } from "../crossPaneBubble";

interface Props {
  inv: CodetermInvocation;
  result?: string;
  resolvePaneName?: PaneNameResolver;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
}

const COLLAPSE_LINES = 8;

function bodyLineCount(body: string): number {
  return body.split("\n").length;
}

export function SentMessageBubble({ inv, result, resolvePaneName, onOpenPlan, resolvePlanRef, fontScale }: Props) {
  const msg = parseSend(inv, result, resolvePaneName);
  const collapsible = bodyLineCount(msg.body) > COLLAPSE_LINES;
  const [expanded, setExpanded] = React.useState(false);
  const showFull = expanded || !collapsible;
  const shown = showFull ? msg.body : msg.body.split("\n").slice(0, COLLAPSE_LINES).join("\n");
  const bodyText = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);

  const header = (
    <div
      data-testid="chat-sent-header"
      className="mb-1 flex items-center gap-1.5 text-xs uppercase tracking-wider"
      style={{ color: "var(--ct-accent, #58a6ff)" }}
    >
      <span className="flex-shrink-0" style={{ opacity: 0.85 }}>
        {msg.mesh ? "🛰" : "→"}
      </span>
      <span className="truncate">
        {msg.recipient}
        {msg.mesh ? ` @ ${msg.mesh}` : ""}
      </span>
      {msg.status && (
        <span
          className="flex-shrink-0 normal-case"
          title={msg.status.label}
          style={{ color: msg.status.ok ? "var(--ct-ok, #41d18a)" : "var(--ct-err, #ff6b6b)" }}
        >
          {msg.status.ok ? "✓" : "✗"}
        </span>
      )}
    </div>
  );

  return (
    <CrossPaneMessageShell
      header={header}
      textSize={bodyText}
      bubbleTestId="chat-sent-message"
      afterBubble={
        collapsible ? (
          <button
            type="button"
            data-testid="chat-sent-expand"
            onClick={() => setExpanded((v) => !v)}
            className="mt-1 text-[11px] underline"
            style={{ color: "var(--ct-accent, #6ea8fe)" }}
          >
            {expanded ? "Show less" : `Show ${bodyLineCount(msg.body) - COLLAPSE_LINES} more lines`}
          </button>
        ) : null
      }
    >
      {hasPlanRefs(shown) ? (
        <LinkedText text={shown} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
      ) : (
        <RichMarkdown text={shown} fontScale={fontScale} />
      )}
    </CrossPaneMessageShell>
  );
}
