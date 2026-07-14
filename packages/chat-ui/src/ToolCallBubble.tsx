import type { NormalizedChatMessage as ChatMessage, ToolEvent } from "@codeterm/chat-core";
import { materializeToolEvent } from "@codeterm/chat-core";
import { CommandCard } from "./toolcard/CommandCard";
import { EditCard } from "./toolcard/EditCard";
import { GenericToolCard } from "./toolcard/GenericToolCard";

interface ToolCallBubbleProps {
  event?: ToolEvent;
  message?: ChatMessage;
  expanded: boolean;
  onToggle: (next: boolean) => void;
  [key: string]: unknown;
}

export function ToolCallBubble({ event: suppliedEvent, message, expanded, onToggle }: ToolCallBubbleProps) {
  const event = suppliedEvent ?? (message ? materializeToolEvent(message) : null);
  if (!event) return null;
  const props = { event, expanded, onToggle: () => onToggle(!expanded) };
  if (event.kind === "command" || event.kind === "codeterm") return <CommandCard {...props} />;
  if (event.kind === "edit") return <EditCard {...props} />;
  return <GenericToolCard {...props} />;
}
