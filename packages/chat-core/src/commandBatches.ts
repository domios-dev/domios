import { toolCommand, toolCommandLabel } from "./shellComposition";
import type { TimelineItem, ToolEvent } from "./toolEvents";

function batchKey(event: ToolEvent): string | undefined {
  const batch = event.message.toolInput?.commandBatchId;
  return event.kind === "command" && event.phase === "call" && typeof batch === "string" && batch.trim()
    ? JSON.stringify([event.message.sourceSessionId, event.flowId, batch]) : undefined;
}

function batchEvent(members: ToolEvent[]): ToolEvent {
  const first = members[0];
  const failed = members.find((member) => member.status === "error");
  const status = members.some((member) => member.status === "pending") ? "pending"
    : failed ? "error" : members.some((member) => member.status === "unresolved") ? "unresolved"
      : members.some((member) => member.status === "interrupted") ? "interrupted" : "success";
  return {
    ...first,
    id: `${first.id}:commands`,
    commandBatch: members,
    status,
    title: toolCommandLabel(first),
    detail: members.map((member) => toolCommand(member).command).join("\n\n"),
    result: members.filter((member) => member.result).map((member) => `${toolCommandLabel(member)}\n${member.result}`).join("\n\n") || undefined,
    exitCode: failed?.exitCode ?? (status === "success" ? 0 : undefined),
    durationMs: undefined,
    error: failed ? true : undefined,
  };
}

export function groupCommandBatches(items: TimelineItem[]): TimelineItem[] {
  const result: TimelineItem[] = [];
  let members: ToolEvent[] = [];
  let key: string | undefined;
  const flush = () => {
    if (members.length) result.push({ kind: "tool", event: members.length > 1 ? batchEvent(members) : members[0] });
    members = [];
  };
  for (const item of items) {
    const nextKey = item.kind === "tool" ? batchKey(item.event) : undefined;
    if (nextKey && item.kind === "tool") {
      if (nextKey !== key) flush();
      members.push(item.event);
      key = nextKey;
    } else {
      flush();
      key = undefined;
      result.push(item);
    }
  }
  flush();
  return result;
}
