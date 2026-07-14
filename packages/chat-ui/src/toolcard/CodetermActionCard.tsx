import type { ToolEvent } from "@codeterm/chat-core";
import { recognize } from "../codeterm/recognize";
import { describeAction, isSend, navigationTarget, type NavigationTarget, type PaneNameResolver } from "../codeterm/templates";
import { SentMessageBubble } from "../codeterm/SentMessageBubble";
import { ToolCardFrame } from "./ToolCardFrame";
import type { OpenPlan, ResolvedChatPlanRef } from "../codeterm/LinkedText";
import { ToolOutputContent } from "../ToolOutputContent";
import { Bot, BookOpen, Brain, Clock3, FileText, Grid3X3, Map, Network, Puzzle, Send, Terminal } from "lucide-react";
import type { ToolKindVisual } from "./toolKindVisual";

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

function actionVisual(path: string[]): ToolKindVisual {
  const family = path[0];
  if (family === "send") return { glyph: Send, color: "var(--ct-accent)", label: "send" };
  if (family === "plan") return { glyph: Map, color: "var(--ct-accent)", label: "plan" };
  if (family === "spec") return { glyph: FileText, color: "var(--ct-cyan)", label: "spec" };
  if (family === "agent") return { glyph: Bot, color: "var(--ct-magenta)", label: "agent" };
  if (family === "mem") return { glyph: Brain, color: "var(--ct-yellow)", label: "mem" };
  if (family === "mesh") return { glyph: Network, color: "var(--ct-cyan)", label: "mesh" };
  if (family === "tab") return { glyph: Terminal, color: "var(--ct-bright-blue)", label: "tab" };
  if (family === "workspace") return { glyph: Grid3X3, color: "var(--ct-muted-fg)", label: "workspace" };
  if (family === "cron" || family === "keepawake") return { glyph: Clock3, color: "var(--ct-magenta)", label: family };
  if (family === "docs") return { glyph: BookOpen, color: "var(--ct-muted-fg)", label: "docs" };
  if (family === "plugin") return { glyph: Puzzle, color: "var(--ct-bright-yellow)", label: "plugin" };
  return { glyph: Terminal, color: "var(--ct-accent)", label: "codeterm" };
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
    summary: descriptor.object ?? descriptor.lead,
    detail: undefined,
  };

  return (
    <ToolCardFrame
      event={displayEvent}
      expanded={expanded}
      onToggle={onToggle}
      title={descriptor.lead}
      summary={descriptor.object}
      variant="codeterm"
      visualOverride={inv ? actionVisual(inv.path) : undefined}
      command={<div data-testid="toolcard-command" className="border-t px-3 py-2 whitespace-pre-wrap break-words font-mono text-xs" style={{ borderColor: "var(--ct-border)", color: "var(--ct-fg)", background: "var(--ct-bg)" }}>{event.detail || event.summary}</div>}
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
      {canNavigate && target?.kind === "tab" && (
        <button
          type="button"
          data-testid="codeterm-focus-tab"
          title="Focus Tab"
          onClick={() => onFocusPane!(target.id)}
          className="mt-2 rounded-md border px-2.5 py-1 font-mono text-[11.5px]"
          style={{ borderColor: "color-mix(in srgb, var(--ct-accent) 32%, transparent)", color: "var(--ct-accent)", background: "color-mix(in srgb, var(--ct-accent) 12%, transparent)" }}
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
          className="mt-2 rounded-md border px-2.5 py-1 font-mono text-[11.5px]"
          style={{ borderColor: "color-mix(in srgb, var(--ct-accent) 32%, transparent)", color: "var(--ct-accent)", background: "color-mix(in srgb, var(--ct-accent) 12%, transparent)" }}
        >
          {`Show ${targetLabel(target)}`}
        </button>
      )}
    </ToolCardFrame>
  );
}
