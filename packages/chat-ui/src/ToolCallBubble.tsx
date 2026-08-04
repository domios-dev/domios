import type { NormalizedChatMessage as ChatMessage, ToolEvent } from "@codeterm/chat-core";
import { materializeToolEvent } from "@codeterm/chat-core";
import type { PeerNameResolver, TabNameResolver } from "./codeterm/templates";
import type { OpenPlan, ResolvedChatPlanRef } from "./codeterm/LinkedText";
import { recognize } from "./codeterm/recognize";
import { CodetermActionCard } from "./toolcard/CodetermActionCard";
import { CommandCard } from "./toolcard/CommandCard";
import { EditCard } from "./toolcard/EditCard";
import { FetchCard } from "./toolcard/FetchCard";
import { GenericCard } from "./toolcard/GenericCard";
import { ReadCard } from "./toolcard/ReadCard";
import { SearchCard } from "./toolcard/SearchCard";
import { TaskCard } from "./toolcard/TaskCard";
import { WaitBatchCard } from "./toolcard/WaitBatchCard";

type PlanFocusKind = "plan" | "task" | "section";

interface ToolCallBubbleProps {
  event?: ToolEvent;
  message?: ChatMessage;
  expanded: boolean;
  onToggle: (next: boolean) => void;
  resolveTabName?: TabNameResolver;
  resolvePeerName?: PeerNameResolver;
  onFocusTab?: (tabRef: string) => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: PlanFocusKind) => ResolvedChatPlanRef | null;
  fontScale?: number;
}

function commandText(event: ToolEvent): string {
  return event.detail?.trim() || event.summary || event.result?.trim() || "";
}

const SEARCH_STAGE = /^(?:rg|grep)\b/;
const READ_STAGE = /^(?:cat|head|tail|nl)\b|^sed\s+-n\b/;

function shellCommandKind(command: string): "read" | "search" | null {
  const segments = command
    .split(/&&|\n/)
    .map((segment) => segment.trim())
    .filter((segment) => segment && !/^set\s+-[a-z]+$/i.test(segment));
  if (segments.length === 0) return null;
  const startsWith = (segment: string, pattern: RegExp) => pattern.test(segment.replace(/^(?:command\s+|env\s+(?:\S+=\S+\s+)*)/, ""));
  // A pipeline's effect is its whole chain, not its first stage: `cat x | codeterm
  // mem save` writes. Classify only when every stage is itself a reader/searcher.
  const stages = (segment: string) => segment.split("|").map((stage) => stage.trim()).filter(Boolean);
  const readerOnly = segments.every((segment) =>
    stages(segment).every((stage) => startsWith(stage, SEARCH_STAGE) || startsWith(stage, READ_STAGE)),
  );
  if (!readerOnly) return null;
  if (segments.every((segment) => startsWith(stages(segment)[0], SEARCH_STAGE))) return "search";
  if (segments.every((segment) => startsWith(stages(segment)[0], READ_STAGE))) return "read";
  return null;
}

export function ToolCallBubble({
  event: suppliedEvent,
  message,
  expanded,
  onToggle,
  resolveTabName,
  resolvePeerName,
  onFocusTab,
  onOpenPlan,
  resolvePlanRef,
  fontScale,
}: ToolCallBubbleProps) {
  const event = suppliedEvent ?? (message ? materializeToolEvent(message) : null);
  if (!event) return null;
  if (event.waitBatch) return <WaitBatchCard event={event} />;

  const presentedKind = event.kind === "command" ? shellCommandKind(commandText(event)) ?? event.kind : event.kind;
  const presentedEvent = presentedKind === event.kind ? event : { ...event, kind: presentedKind };
  const props = { event: presentedEvent, expanded, onToggle: () => onToggle(!expanded) };
  switch (presentedKind) {
    case "command":
      return recognize(commandText(event)) ? (
        <CodetermActionCard
          {...props}
          resolveTabName={resolveTabName}
          resolvePeerName={resolvePeerName}
          onFocusTab={onFocusTab}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
      ) : <CommandCard {...props} />;
    case "codeterm":
      return (
        <CodetermActionCard
          {...props}
          resolveTabName={resolveTabName}
          resolvePeerName={resolvePeerName}
          onFocusTab={onFocusTab}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
      );
    case "edit": return <EditCard {...props} />;
    case "read": return <ReadCard {...props} />;
    case "search": return <SearchCard {...props} />;
    case "task": return <TaskCard {...props} />;
    case "fetch": return <FetchCard {...props} />;
    case "generic": return <GenericCard {...props} />;
  }
}
