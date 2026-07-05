import React from "react";
import type { CodetermInvocation } from "./recognize";
import { parseSend, type PaneNameResolver } from "./templates";
import { RichMarkdown } from "../MarkdownBody";
import { LinkedText, type OpenPlan } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";

interface Props {
  inv: CodetermInvocation;
  result?: string;
  resolvePaneName?: PaneNameResolver;
  onOpenPlan?: OpenPlan;
  fontScale?: number;
}

const COLLAPSE_LINES = 8;

function bodyLineCount(body: string): number {
  return body.split("\n").length;
}

export function SentMessageBubble({ inv, result, resolvePaneName, onOpenPlan, fontScale }: Props) {
  const msg = parseSend(inv, result, resolvePaneName);
  const collapsible = bodyLineCount(msg.body) > COLLAPSE_LINES;
  const [expanded, setExpanded] = React.useState(false);
  const showFull = expanded || !collapsible;
  const shown = showFull ? msg.body : msg.body.split("\n").slice(0, COLLAPSE_LINES).join("\n");

  return (
    <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto flex justify-end">
      <div
        data-testid="chat-sent-message"
        className="min-w-0 max-w-[min(44rem,90%)] rounded-lg border px-3 py-2"
        style={{
          background: "color-mix(in srgb, var(--ct-accent, #6ea8fe) 8%, transparent)",
          borderColor: "color-mix(in srgb, var(--ct-accent, #6ea8fe) 30%, transparent)",
          borderLeft: "3px solid var(--ct-accent, #6ea8fe)",
        }}
      >
        <div className="flex items-center gap-1.5 text-[11px] mb-1" style={{ color: "var(--ct-muted-fg)" }}>
          <span style={{ color: "var(--ct-accent, #6ea8fe)" }}>{msg.mesh ? "🛰" : "→"}</span>
          <span className="font-medium truncate" style={{ color: "var(--ct-fg)" }}>
            {msg.recipient}
          </span>
          {msg.mesh && <span className="truncate">@ {msg.mesh}</span>}
          {msg.status && (
            <span
              className="flex-shrink-0"
              title={msg.status.label}
              style={{ color: msg.status.ok ? "var(--ct-ok, #41d18a)" : "var(--ct-err, #ff6b6b)" }}
            >
              {msg.status.ok ? "✓" : "✗"}
            </span>
          )}
        </div>
        <div className="text-xs" style={{ color: "var(--ct-fg)" }}>
          {hasPlanRefs(shown) ? (
            <LinkedText text={shown} onOpenPlan={onOpenPlan} />
          ) : (
            <RichMarkdown text={shown} fontScale={fontScale} />
          )}
        </div>
        {collapsible && (
          <button
            type="button"
            data-testid="chat-sent-expand"
            onClick={() => setExpanded((v) => !v)}
            className="mt-1 text-[11px] underline"
            style={{ color: "var(--ct-accent, #6ea8fe)" }}
          >
            {expanded ? "Show less" : `Show ${bodyLineCount(msg.body) - COLLAPSE_LINES} more lines`}
          </button>
        )}
      </div>
    </div>
  );
}
