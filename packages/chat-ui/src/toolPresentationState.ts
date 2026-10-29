import type { NormalizedChatMessage, TimelineItem, ToolEvent } from "@codeterm/chat-core";

const RETAINED_COMMAND_LIMIT = 4096;
const retainedCommands = new Map<string, true>();

export function toolPresentationKey(message: NormalizedChatMessage, item?: TimelineItem): string {
  if (item?.kind !== "tool") return message.id;
  const session = message.sourceSessionId ?? "";
  if (item.event.stream) return `stream:${session}:${item.event.stream.flowId}`;
  const toolId = item.event.toolId?.trim();
  return toolId ? `tool:${session}:${toolId}` : message.id;
}

export function retainStreamedCommandShell(event: ToolEvent): boolean {
  if (event.kind !== "command" || event.stream || event.commandBatch || event.agents) return false;
  const key = toolPresentationKey(event.message, { kind: "tool", event });
  if (event.status !== "pending" && !retainedCommands.has(key)) return false;
  retainedCommands.delete(key);
  retainedCommands.set(key, true);
  if (retainedCommands.size > RETAINED_COMMAND_LIMIT) retainedCommands.delete(retainedCommands.keys().next().value!);
  return true;
}
