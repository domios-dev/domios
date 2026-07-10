import React from "react";
import type { CodetermInvocation } from "./recognize";
import { renderTemplate, isSend, type PaneNameResolver } from "./templates";
import { SentMessageBubble } from "./SentMessageBubble";
import { LinkedText, type OpenPlan, type ResolvedChatPlanRef } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";

interface Props {
  inv: CodetermInvocation;
  result?: string;
  expanded: boolean;
  onToggle: () => void;
  resolvePaneName?: PaneNameResolver;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
}

export function CodetermCard({ inv, result, expanded, onToggle, resolvePaneName, onOpenPlan, resolvePlanRef, fontScale }: Props) {
  if (isSend(inv)) {
    return (
      <SentMessageBubble
        inv={inv}
        result={result}
        resolvePaneName={resolvePaneName}
        onOpenPlan={onOpenPlan}
        resolvePlanRef={resolvePlanRef}
        fontScale={fontScale}
      />
    );
  }

  const card = renderTemplate(inv, result, resolvePaneName);
  const canOpenPlan = card.link != null && onOpenPlan != null;
  return (
    <React.Fragment>
      <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
        <div className="inline-flex flex-col max-w-[min(52rem,100%)]">
          <button
            onClick={onToggle}
            className="inline-flex min-w-0 items-center gap-1.5 text-left rounded-md border px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
            style={{ background: "transparent", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
          >
            <span className="flex-shrink-0" style={{ color: "var(--ct-accent, #6ea8fe)" }}>
              ⬡
            </span>
            <span className="font-medium" style={{ color: "var(--ct-accent, #6ea8fe)" }}>
              CodeTerm
            </span>
            <span className="flex-shrink-0">{card.icon}</span>
            <span className="truncate">{card.title}</span>
            {card.status && (
              <span
                className="flex-shrink-0 whitespace-nowrap rounded px-1.5 py-0.5 text-[10px]"
                style={{
                  background: card.status.ok
                    ? "color-mix(in srgb, var(--ct-ok, #41d18a) 10%, transparent)"
                    : "color-mix(in srgb, var(--ct-err, #ff6b6b) 10%, transparent)",
                  color: card.status.ok ? "var(--ct-ok, #41d18a)" : "var(--ct-err, #ff6b6b)",
                }}
              >
                {card.status.ok ? "✓" : "✗"} {card.status.label}
              </span>
            )}
          </button>
          {canOpenPlan && (
            <button
              type="button"
              data-testid="chat-open-plan"
              data-plan-id={card.link!.planId}
              data-task-id={card.link!.taskId}
              onClick={() => onOpenPlan!(card.link!.planId, card.link!.taskId)}
              className="mt-0.5 self-start text-[11px] underline"
              style={{ color: "var(--ct-accent, #6ea8fe)" }}
            >
              open in plan →
            </button>
          )}
          {card.detail && !expanded && (
            <div className="px-2.5 py-1 text-xs truncate" style={{ color: "var(--ct-muted-fg)" }}>
              {hasPlanRefs(card.detail) ? (
                <LinkedText text={card.detail} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} />
              ) : (
                card.detail
              )}
            </div>
          )}
          {expanded && (
            <div className="mt-1 rounded-md border px-2.5 py-1.5 text-xs" style={{ borderColor: "var(--ct-border)" }}>
              <pre className="whitespace-pre-wrap break-all" style={{ color: "var(--ct-muted-fg)" }}>
                {inv.raw}
              </pre>
              {result && <pre className="mt-1 whitespace-pre-wrap break-all" style={{ color: "var(--ct-fg)" }}>{result}</pre>}
            </div>
          )}
        </div>
      </div>
    </React.Fragment>
  );
}
