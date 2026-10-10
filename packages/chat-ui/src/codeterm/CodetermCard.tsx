import type { CSSProperties } from "react";
import { actionVisual } from "../toolcard/CodetermActionCard";
import { ToolCardActionButton } from "../toolcard/ToolCardFrame";
import type { CodetermInvocation } from "./recognize";
import { renderTemplate, isSend, type TabNameResolver } from "./templates";
import { SentMessageBubble } from "./SentMessageBubble";
import { LinkedText, type OpenPlan, type ResolvedChatPlanRef } from "./LinkedText";
import { hasPlanRefs } from "./planRefs";

interface Props {
  inv: CodetermInvocation;
  result?: string;
  expanded: boolean;
  onToggle: () => void;
  resolveTabName?: TabNameResolver;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: "plan" | "task" | "section") => ResolvedChatPlanRef | null;
  fontScale?: number;
}

export function CodetermCard({ inv, result, expanded, onToggle, resolveTabName, onOpenPlan, resolvePlanRef, fontScale }: Props) {
  if (isSend(inv)) {
    return (
      <SentMessageBubble
        inv={inv}
        result={result}
        resolveTabName={resolveTabName}
        onOpenPlan={onOpenPlan}
        resolvePlanRef={resolvePlanRef}
        fontScale={fontScale}
      />
    );
  }

  const card = renderTemplate(inv, result, resolveTabName);
  const canOpenPlan = card.link != null && onOpenPlan != null;
  const visual = actionVisual(inv);
  const Glyph = visual.glyph;
  const showDetail = Boolean(card.detail) && !expanded;
  return (
    <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
      <div className={`ct-card${expanded ? "" : " ct-card--quiet"}`} style={{ "--ct-card-tone": visual.color } as CSSProperties}>
        <button type="button" aria-expanded={expanded} onClick={onToggle} className="ct-card__head">
          <span className="ct-card__icon"><Glyph size={14} aria-hidden="true" /></span>
          <span className="ct-card__title min-w-0 flex-1"><span className="truncate">{card.title}</span></span>
          {card.status && (
            <span className="ct-card__chip" style={{ color: card.status.ok ? "var(--ct-ok)" : "var(--ct-err)" }}>
              {card.status.label}
            </span>
          )}
        </button>
        {(showDetail || canOpenPlan || expanded) && (
          <div className="ct-card__body grid gap-1.5">
            {showDetail && (
              <div className="truncate" style={{ color: "var(--ct-muted-fg)" }}>
                {hasPlanRefs(card.detail!) ? <LinkedText text={card.detail!} onOpenPlan={onOpenPlan} resolvePlanRef={resolvePlanRef} /> : card.detail}
              </div>
            )}
            {expanded && (
              <div className="ct-card__body--reveal grid gap-1 font-mono text-[11px]">
                <pre className="m-0 whitespace-pre-wrap break-all" style={{ color: "var(--ct-muted-fg)" }}>{inv.raw}</pre>
                {result && <pre className="m-0 whitespace-pre-wrap break-all" style={{ color: "var(--ct-fg)" }}>{result}</pre>}
              </div>
            )}
            {canOpenPlan && (
              <ToolCardActionButton primary testId="chat-open-plan" label="Open in plan" onClick={() => onOpenPlan!(card.link!.planId, card.link!.taskId)} />
            )}
          </div>
        )}
      </div>
    </div>
  );
}
