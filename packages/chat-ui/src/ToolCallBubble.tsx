import type { NormalizedChatMessage as ChatMessage, ToolEvent } from "@codeterm/chat-core";
import { materializeToolEvent } from "@codeterm/chat-core";
import type { PaneNameResolver } from "./codeterm/templates";
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
  resolvePaneName?: PaneNameResolver;
  onFocusPane?: (paneRef: string) => void;
  onOpenPlan?: OpenPlan;
  resolvePlanRef?: (planId: string, focusId?: string, focusKind?: PlanFocusKind) => ResolvedChatPlanRef | null;
  fontScale?: number;
}

function commandText(event: ToolEvent): string {
  return event.detail?.trim() || event.summary || event.result?.trim() || "";
}

export function ToolCallBubble({
  event: suppliedEvent,
  message,
  expanded,
  onToggle,
  resolvePaneName,
  onFocusPane,
  onOpenPlan,
  resolvePlanRef,
  fontScale,
}: ToolCallBubbleProps) {
  const event = suppliedEvent ?? (message ? materializeToolEvent(message) : null);
  if (!event) return null;
  if (event.waitBatch) return <WaitBatchCard event={event} />;

  const props = { event, expanded, onToggle: () => onToggle(!expanded) };
  switch (event.kind) {
    case "command":
      return recognize(commandText(event)) ? (
        <CodetermActionCard
          {...props}
          resolvePaneName={resolvePaneName}
          onFocusPane={onFocusPane}
          onOpenPlan={onOpenPlan}
          resolvePlanRef={resolvePlanRef}
          fontScale={fontScale}
        />
      ) : <CommandCard {...props} />;
    case "codeterm":
      return (
        <CodetermActionCard
          {...props}
          resolvePaneName={resolvePaneName}
          onFocusPane={onFocusPane}
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
