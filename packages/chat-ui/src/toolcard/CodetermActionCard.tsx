import type { ToolEvent } from "@codeterm/chat-core";
import { recognize } from "../codeterm/recognize";
import { describeAction, isHelp, isSend, navigationTarget, parseSend, type NavigationTarget, type PeerNameResolver, type TabNameResolver } from "../codeterm/templates";
import { CompactMessageContent } from "../codeterm/CompactMessageContent";
import { ToolCardActionButton, ToolCardFrame } from "./ToolCardFrame";
import type { OpenPlan, ResolvedChatPlanRef } from "../codeterm/LinkedText";
import { ToolOutputContent } from "../ToolOutputContent";
import { TabListCard } from "./TabListCard";
import {
  Activity, Bot, BookOpen, Brain, CalendarClock, CheckCircle2, CircleX, Clock3, Coffee,
  FilePenLine, FilePlus2, FileText, FolderKanban, GitBranch, Grid3X3, History,
  Link2, ListChecks, ListTree, Map, MessageSquareText, MousePointer2, Network, Play,
  Puzzle, RadioTower, ScrollText, Search, Send, Sparkles, SquarePlus, Sunrise, Terminal, Users,
} from "lucide-react";
import type { ToolKindVisual } from "./toolKindVisual";

type PlanFocusKind = "plan" | "task" | "section";

interface Props {
  event: ToolEvent;
  expanded: boolean;
  onToggle: () => void;
  resolveTabName?: TabNameResolver;
  resolvePeerName?: PeerNameResolver;
  onFocusTab?: (tabRef: string) => void;
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

function actionVisual(invocation: NonNullable<ReturnType<typeof recognize>>): ToolKindVisual {
  const { path } = invocation;
  const command = path.join(" ");
  if (isHelp(invocation)) {
    return { glyph: BookOpen, color: "var(--ct-muted-fg)", label: "help" };
  }
  if (command === "send" && typeof invocation.flags.mesh === "string") {
    return { glyph: RadioTower, color: "var(--ct-cyan)", label: "mesh send" };
  }
  const exact: Record<string, ToolKindVisual> = {
    "plan create": { glyph: Map, color: "var(--ct-accent)", label: "plan" },
    "tab get-text": { glyph: ScrollText, color: "var(--ct-cyan)", label: "tab output" },
    "tab new": { glyph: SquarePlus, color: "var(--ct-bright-blue)", label: "new tab" },
    "tab list": { glyph: ListTree, color: "var(--ct-bright-blue)", label: "tab list" },
    "tab focus": { glyph: MousePointer2, color: "var(--ct-bright-blue)", label: "focus tab" },
    "tab exec": { glyph: Play, color: "var(--ct-green)", label: "tab exec" },
    "tab close": { glyph: CircleX, color: "var(--ct-red)", label: "close tab" },
    "tab search": { glyph: Search, color: "var(--ct-cyan)", label: "tab search" },
    "tab status": { glyph: Activity, color: "var(--ct-green)", label: "tab status" },
    "workspace focus": { glyph: FolderKanban, color: "var(--ct-bright-blue)", label: "workspace" },
    "plan goal set": { glyph: GitBranch, color: "var(--ct-accent)", label: "goal" },
    "plan section add": { glyph: FilePlus2, color: "var(--ct-accent)", label: "section" },
    "plan section update": { glyph: FilePenLine, color: "var(--ct-accent)", label: "section edit" },
    "plan task add": { glyph: ListChecks, color: "var(--ct-accent)", label: "plan task" },
    "plan task update": { glyph: FilePenLine, color: "var(--ct-accent)", label: "task edit" },
    "plan task close": { glyph: ListChecks, color: "var(--ct-green)", label: "close task" },
    "plan retrospective": { glyph: History, color: "var(--ct-magenta)", label: "retrospective" },
    "plan record": { glyph: MessageSquareText, color: "var(--ct-accent)", label: "decision" },
    "plan skill new": { glyph: Sparkles, color: "var(--ct-magenta)", label: "skill" },
    "plan state": { glyph: Activity, color: "var(--ct-green)", label: "state" },
    "spec create": { glyph: FilePlus2, color: "var(--ct-cyan)", label: "spec" },
    "spec section add": { glyph: FileText, color: "var(--ct-cyan)", label: "spec section" },
    "spec link add": { glyph: Link2, color: "var(--ct-cyan)", label: "spec link" },
    "agent spawn": { glyph: Bot, color: "var(--ct-magenta)", label: "spawn" },
    "orchestrator status": { glyph: Users, color: "var(--ct-magenta)", label: "agents" },
    "agent report": { glyph: MessageSquareText, color: "var(--ct-magenta)", label: "report" },
    "mesh peers": { glyph: Network, color: "var(--ct-cyan)", label: "peers" },
    "mem save": { glyph: Brain, color: "var(--ct-yellow)", label: "mem save" },
    "mem search": { glyph: Search, color: "var(--ct-yellow)", label: "mem search" },
    "mem wake-up": { glyph: Sunrise, color: "var(--ct-yellow)", label: "wake-up" },
    "mem get": { glyph: BookOpen, color: "var(--ct-yellow)", label: "mem get" },
    "cron create": { glyph: Clock3, color: "var(--ct-magenta)", label: "cron" },
    "cron list": { glyph: CalendarClock, color: "var(--ct-magenta)", label: "crons" },
    "keepawake start": { glyph: Coffee, color: "var(--ct-green)", label: "keepawake" },
    "keepawake status": { glyph: Coffee, color: "var(--ct-green)", label: "status" },
    docs: { glyph: BookOpen, color: "var(--ct-muted-fg)", label: "docs" },
    "plugin list": { glyph: Puzzle, color: "var(--ct-bright-yellow)", label: "plugins" },
    "plan approve": { glyph: CheckCircle2, color: "var(--ct-green)", label: "approved" },
  };
  if (exact[command]) return exact[command];
  const family = path[0];
  if (family === "send") return { glyph: Send, color: "var(--ct-accent)", label: "send" };
  if (family === "plan") return { glyph: Map, color: "var(--ct-accent)", label: "plan" };
  if (family === "spec") return { glyph: FileText, color: "var(--ct-cyan)", label: "spec" };
  if (family === "agent") return { glyph: Bot, color: "var(--ct-magenta)", label: "agent" };
  if (family === "mem") return { glyph: Brain, color: "var(--ct-yellow)", label: "mem" };
  if (family === "mesh") return { glyph: Network, color: "var(--ct-cyan)", label: "mesh" };
  if (family === "tab") return { glyph: Grid3X3, color: "var(--ct-bright-blue)", label: path.join(" ") };
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
  resolveTabName,
  resolvePeerName,
  onFocusTab,
  onOpenPlan,
  resolvePlanRef,
  fontScale,
  onOpenSpec,
  onOpenMemory,
  onOpenSource,
}: Props) {
  const inv = recognize(commandFor(event));
  const sent = inv && isSend(inv) ? parseSend(inv, event.result, resolveTabName, resolvePeerName) : undefined;
  const sendSucceeded = sent?.status?.ok === true;
  const target = inv ? navigationTarget(inv, event.result) : null;
  const descriptor = inv ? describeAction(inv, event.result, resolveTabName, resolvePeerName) : { lead: "codeterm", chips: [] };
  const collapsedChips = descriptor.chips.filter((chip) => !/\b(?:pl|task|sec|sp|spec|mem|src)_[a-z0-9][a-z0-9_-]*/i.test(chip));
  const canNavigate = target != null && (
    target.kind === "tab" ? Boolean(onFocusTab) :
    target.kind === "plan" || target.kind === "task" || target.kind === "section" || target.kind === "goal"
      ? Boolean(onOpenPlan && target.planId)
      : target.kind === "spec" ? Boolean(onOpenSpec) : target.kind === "memory" ? Boolean(onOpenMemory) : Boolean(onOpenSource)
  );
  const displayEvent: ToolEvent = {
    ...event,
    status: sendSucceeded ? "success" : event.status,
    summary: descriptor.object ?? descriptor.lead,
    detail: undefined,
  };
  const isTabList = inv?.path.join(" ") === "tab list";
  const tabListJson = isTabList && inv?.flags.json === true;
  const showResult = Boolean(event.result && (!isTabList || tabListJson) && (!inv || !isSend(inv) || (event.status === "error" && !sendSucceeded)));

  return (
    <ToolCardFrame
      event={displayEvent}
      expanded={expanded}
      onToggle={onToggle}
      title={descriptor.lead}
      summary={descriptor.object}
      variant="codeterm"
      disclosureLabel="full command"
      visualOverride={inv ? actionVisual(inv) : undefined}
      metric={collapsedChips.length ? <span className="flex flex-wrap gap-1">{collapsedChips.map((chip) => <span key={chip} className="rounded border px-1.5 py-0.5 text-[10px]" style={{ borderColor: "var(--ct-border)", color: "var(--ct-muted-fg)" }}>{chip}</span>)}</span> : undefined}
      action={canNavigate && target?.kind === "tab" ? <ToolCardActionButton label="Show Tab" onClick={() => onFocusTab!(target.id)} testId="codeterm-focus-tab" /> : undefined}
    >
      {sent ? (
        <CompactMessageContent
          text={sent.formattedAvailable ? sent.body : event.detail || event.summary}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
      ) : isTabList && !tabListJson ? (
        <TabListCard event={event} />
      ) : descriptor.fields?.length ? (
        <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
          {descriptor.fields.map((field) => (
            <div key={field.label} className="contents">
              <dt className="font-mono" style={{ color: "var(--ct-muted-fg)" }}>{field.label}</dt>
              <dd className="min-w-0 break-words" style={{ color: "var(--ct-fg)" }}>{field.value}</dd>
            </div>
          ))}
        </dl>
      ) : descriptor.detail ? (
        <div className="break-words text-xs" style={{ color: "var(--ct-muted-fg)" }}>{descriptor.detail}</div>
      ) : null}
      {descriptor.bullets && <ul className="mt-2 list-disc pl-4 text-xs" style={{ color: "var(--ct-muted-fg)" }}>{descriptor.bullets.map((bullet) => <li key={bullet}>{bullet}</li>)}</ul>}
      {showResult && <div data-testid={tabListJson ? "tab-list-json" : "toolcard-result"} className="mt-2"><ToolOutputContent text={event.result!} maxHeight="12rem" /></div>}
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
