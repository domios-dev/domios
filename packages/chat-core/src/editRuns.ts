import type { TimelineItem, ToolEvent } from "./toolEvents";

/** A standalone structured edit that can sit in a run; streamed, batched and grouped events keep their own card. */
function runKey(event: ToolEvent): string | undefined {
  if (event.kind !== "edit" || event.phase !== "call" || !event.fileDiffs?.length) return undefined;
  if (event.stream || event.commandBatch || event.agents || event.editRun) return undefined;
  return event.message.sourceSessionId ?? "";
}

function runEvent(members: ToolEvent[]): ToolEvent {
  const first = members[0];
  const status = members.some((member) => member.status === "pending") ? "pending"
    : members.some((member) => member.status === "error") ? "error"
      : members.some((member) => member.status === "unresolved") ? "unresolved"
        : members.some((member) => member.status === "interrupted") ? "interrupted" : "success";
  const { fileDiffs: _fileDiffs, edits: _edits, ...head } = first;
  return {
    ...head,
    id: `${first.id}:edits`,
    editRun: members,
    status,
    result: undefined,
    error: members.some((member) => member.status === "error") ? true : undefined,
  };
}

/** Folds back-to-back file edits of one session into a single run, so a burst of edits reads as one tree. */
export function groupEditRuns(items: TimelineItem[]): TimelineItem[] {
  const result: TimelineItem[] = [];
  let members: ToolEvent[] = [];
  let key: string | undefined;
  const flush = () => {
    if (members.length) result.push({ kind: "tool", event: members.length > 1 ? runEvent(members) : members[0] });
    members = [];
    key = undefined;
  };
  for (const item of items) {
    const nextKey = item.kind === "tool" ? runKey(item.event) : undefined;
    if (nextKey === undefined || item.kind !== "tool") {
      flush();
      result.push(item);
      continue;
    }
    if (nextKey !== key) flush();
    members.push(item.event);
    key = nextKey;
  }
  flush();
  return result;
}
