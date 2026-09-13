import type { NormalizedChatMessage as ChatMessage, ToolEvent } from "@codeterm/chat-core";
import { materializeToolEvent } from "@codeterm/chat-core";
import type { PeerNameResolver, TabNameResolver } from "./codeterm/templates";
import type { OpenPlan, ResolvedChatPlanRef } from "./codeterm/LinkedText";
import { toolCommandShape } from "./codeterm/commandShape";
import { AgentActivityCard } from "./toolcard/AgentActivityCard";
import { CodetermActionCard } from "./toolcard/CodetermActionCard";
import { CommandCard } from "./toolcard/CommandCard";
import { CommandGroupCard } from "./toolcard/CommandGroupCard";
import { EditCard } from "./toolcard/EditCard";
import { FetchCard } from "./toolcard/FetchCard";
import { FileDiffCards } from "./toolcard/FileDiffCards";
import { GenericCard } from "./toolcard/GenericCard";
import { ReadCard } from "./toolcard/ReadCard";
import { RichResultCard } from "./toolcard/RichResultCard";
import { SearchCard } from "./toolcard/SearchCard";
import { TaskCard } from "./toolcard/TaskCard";
import { ToolActivityStream } from "./toolcard/ToolActivityStream";

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
  const codetermProps = { resolveTabName, resolvePeerName, onFocusTab, onOpenPlan, resolvePlanRef, fontScale };
  if (event.agents) return <AgentActivityCard event={event} expanded={expanded} onToggle={onToggle} />;
  if (event.stream) {
    return (
      <ToolActivityStream
        event={event}
        expanded={expanded}
        onToggle={onToggle}
        renderAction={(member, memberExpanded, onMemberToggle) => (
          <ToolCallBubble event={member} expanded={memberExpanded} onToggle={onMemberToggle} {...codetermProps} />
        )}
      />
    );
  }

  // A structured source diff is the truthful record of the edit, so it replaces the old/new-string card.
  if (event.fileDiffs?.length) return <FileDiffCards event={event} initiallyExpanded={expanded} />;

  const shape = event.kind === "command" || event.kind === "codeterm" ? toolCommandShape(event) : null;
  if (shape?.kind === "group") {
    return <CommandGroupCard event={event} rows={shape.rows} expanded={expanded} onToggle={onToggle} {...codetermProps} />;
  }
  const toggle = () => onToggle(!expanded);
  if (event.kind === "codeterm" || shape?.kind === "codeterm") {
    const invocation = shape?.kind === "codeterm" ? shape.invocation : undefined;
    return <CodetermActionCard event={event} invocation={invocation} expanded={expanded} onToggle={toggle} {...codetermProps} />;
  }
  if (shape?.kind === "read") return <ReadCard event={{ ...event, kind: "read" }} expanded={expanded} onToggle={toggle} />;
  if (shape?.kind === "search") return <SearchCard event={{ ...event, kind: "search" }} expanded={expanded} onToggle={toggle} />;

  const props = { event, expanded, onToggle: toggle };
  switch (event.kind) {
  case "command": return <CommandCard {...props} />;
  case "edit": return <EditCard {...props} />;
  case "read": return <ReadCard {...props} />;
  case "search": return <SearchCard {...props} />;
  case "task": return <TaskCard {...props} />;
  case "fetch": return <FetchCard {...props} />;
  case "web":
  case "image": return <RichResultCard {...props} />;
  case "generic": return <GenericCard {...props} />;
  default: return null;
  }
}
