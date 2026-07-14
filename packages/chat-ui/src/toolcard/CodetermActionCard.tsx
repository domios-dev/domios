import type { ToolEvent } from "@codeterm/chat-core";
import { recognize } from "../codeterm/recognize";
import { describeAction, isSend, navigationTarget, type NavigationTarget, type PaneNameResolver } from "../codeterm/templates";
import { SentMessageBubble } from "../codeterm/SentMessageBubble";
import { ToolCardFrame } from "./ToolCardFrame";
import type { OpenPlan, ResolvedChatPlanRef } from "../codeterm/LinkedText";
import { ToolOutputContent } from "../ToolOutputContent";

type PlanFocusKind = "plan" | "task" | "section";

interface Props {
  event: ToolEvent;
  expanded: boolean;
  onToggle: () => void;
  resolvePaneName?: PaneNameResolver;
  onFocusPane?: (paneRef: string) => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: PlanFocusKind) => ResolvedChatPlanRef | null;
  onOpenSpec?: (specId: string) => void;
  onOpenMemory?: (memoryId: string) => void;
  onOpenSource?: (sourceId: string) => void;
  fontScale?: number;
}

function commandFor(event: ToolEvent): string {
  return event.detail?.trim() || event.summary || event.result?.trim() || "";
}

function targetLabel(target: NavigationTarget): string {
  if (target.kind === "tab") return "Tab";
  return target.kind[0].toUpperCase() + target.kind.slice(1);
}

export function CodetermActionCard({
  event,
  expanded,
  onToggle,
  resolvePaneName,
  onFocusPane,
  onOpenPlan,
  resolvePlanRef,
  fontScale,
  onOpenSpec,
  onOpenMemory,
  onOpenSource,
}: Props) {
  const inv = recognize(commandFor(event));
  const target = inv ? navigationTarget(inv, event.result) : null;
  const descriptor = inv ? describeAction(inv, event.result, resolvePaneName) : { lead: "codeterm", chips: [] };
  const collapsedChips = descriptor.chips.filter((chip) => !/\b(?:pl|task|sec|sp|spec|mem|src)_[a-z0-9][a-z0-9_-]*/i.test(chip));
  const canNavigate = target != null && (
    target.kind === "tab" ? Boolean(onFocusPane) :
    target.kind === "plan" || target.kind === "task" || target.kind === "section" || target.kind === "goal"
      ? Boolean(onOpenPlan && target.planId)
      : target.kind === "spec" ? Boolean(onOpenSpec) : target.kind === "memory" ? Boolean(onOpenMemory) : Boolean(onOpenSource)
  );
  const displayEvent: ToolEvent = {
    ...event,
    summary: descriptor.object ? `${descriptor.lead} · ${descriptor.object}` : descriptor.lead,
    detail: undefined,
  };

  return (
    <ToolCardFrame
      event={displayEvent}
      expanded={expanded}
      onToggle={onToggle}
      title="codeterm"
      metric={collapsedChips.length ? <span className="flex flex-wrap gap-1">{collapsedChips.map((chip) => <span key={chip} className="rounded border px-1.5 py-0.5 text-[10px]" style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}>{chip}</span>)}</span> : undefined}
    >
      {inv && isSend(inv) ? (
        <SentMessageBubble
          inv={inv}
          result={event.result}
          resolvePaneName={resolvePaneName}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
      ) : (
        <div className="text-xs" style={{ color: "var(--ct-fg)" }}>
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-medium" style={{ color: "var(--ct-accent)" }}>{descriptor.lead}</span>
            {descriptor.object && <span className="truncate" style={{ color: "var(--ct-fg)" }}>{descriptor.object}</span>}
            {descriptor.chips.map((chip) => <span key={chip} className="rounded border px-1.5 py-0.5 text-[10px]" style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}>{chip}</span>)}
          </div>
          {descriptor.detail && <div className="mt-1 truncate" style={{ color: "var(--ct-muted-fg)" }}>{descriptor.detail}</div>}
        </div>
      )}
      {descriptor.bullets && <ul className="mt-2 list-disc pl-4 text-xs" style={{ color: "var(--ct-muted-fg)" }}>{descriptor.bullets.map((bullet) => <li key={bullet}>{bullet}</li>)}</ul>}
      {event.result && <div data-testid="toolcard-result" className="mt-2"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
      <div data-testid="toolcard-command" className="mt-2 whitespace-pre-wrap break-words font-mono text-xs" style={{ color: "var(--ct-muted-fg)" }}>{event.detail || event.summary}</div>
      {canNavigate && target?.kind === "tab" && (
        <button
          type="button"
          data-testid="codeterm-focus-tab"
          title="Focus Tab"
          onClick={() => onFocusPane!(target.id)}
          className="mt-2 rounded border px-2 py-1 text-[11px]"
          style={{ borderColor: "var(--ct-border)", color: "var(--ct-accent)" }}
        >
          Focus Tab
        </button>
      )}
      {canNavigate && target && target.kind !== "tab" && (
        <button
          type="button"
          data-testid="codeterm-show-plan"
          title={`Show ${targetLabel(target)}`}
          onClick={() => {
            if (target.kind === "spec") onOpenSpec!(target.id);
            else if (target.kind === "memory") onOpenMemory!(target.id);
            else if (target.kind === "source") onOpenSource!(target.id);
            else onOpenPlan!(target.planId!, target.kind === "plan" || target.kind === "goal" ? undefined : target.id, target.kind === "goal" ? "plan" : target.kind);
          }}
          className="mt-2 rounded border px-2 py-1 text-[11px]"
          style={{ borderColor: "var(--ct-border)", color: "var(--ct-accent)" }}
        >
          {`Show ${targetLabel(target)}`}
        </button>
      )}
    </ToolCardFrame>
  );
}
