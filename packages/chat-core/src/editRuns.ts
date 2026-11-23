import type { TimelineItem, ToolEvent } from "./toolEvents";
import type { FileDiff } from "./types";

export function isTurnDiff(event: ToolEvent): boolean {
  return event.kind === "edit" && event.phase === "call" && !event.toolId
    && event.message.toolName === "turn_diff";
}

function changeKey(event: ToolEvent, diff: FileDiff): string {
  return JSON.stringify([
    event.message.sourceSessionId ?? "", event.flowId, diff.path.replace(/\\/g, "/"),
    diff.movePath?.replace(/\\/g, "/"), diff.kind ?? "modify",
  ]);
}

function sameChange(recorded: FileDiff, snapshot: FileDiff): boolean {
  return recorded.additions === snapshot.additions && recorded.deletions === snapshot.deletions
    && recorded.hunks.length === snapshot.hunks.length
    && recorded.hunks.every((hunk, index) => {
      const other = snapshot.hunks[index];
      if (hunk.oldStart !== other.oldStart || hunk.newStart !== other.newStart
        || hunk.oldLines !== other.oldLines || hunk.newLines !== other.newLines) return false;
      const stripped = (lines: string[]) => lines.filter((line) => !line.startsWith("\\"));
      const left = stripped(hunk.lines);
      const right = stripped(other.lines);
      return left.length === right.length && left.every((line, i) => line.replace(/\r$/, "") === right[i]);
    });
}

/** A turn snapshot can echo a completed edit. Keep the snapshot's distinct changes and each actual call. */
export function reconcileTurnDiffs(items: TimelineItem[]): Set<number> {
  const suppressed = new Set<number>();
  const snapshots = new Map<number, ToolEvent>();
  items.forEach((item, index) => {
    if (item.kind === "tool" && item.event.flowId && item.event.fileDiffs?.length && isTurnDiff(item.event)) {
      snapshots.set(index, item.event);
    }
  });
  if (!snapshots.size) return suppressed;

  const recorded = new Map<string, FileDiff[]>();
  for (const item of items) {
    if (item.kind !== "tool") continue;
    const { event } = item;
    if (!event.flowId || event.phase !== "call" || event.status !== "success" || isTurnDiff(event)) continue;
    for (const diff of event.fileDiffs ?? []) {
      if (diff.binary || diff.truncated) continue;
      const key = changeKey(event, diff);
      const changes = recorded.get(key);
      if (changes) changes.push(diff);
      else recorded.set(key, [diff]);
    }
  }
  for (const [index, event] of snapshots) {
    const diffs = event.fileDiffs ?? [];
    const remaining = diffs.filter((diff) => !recorded.get(changeKey(event, diff))?.some((change) => sameChange(change, diff)));
    if (!remaining.length) suppressed.add(index);
    else if (remaining.length !== diffs.length) {
      items[index] = { kind: "tool", event: { ...event, fileDiffs: remaining, summary: remaining.map((diff) => diff.path).join("\n") } };
    }
  }
  return suppressed;
}

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
