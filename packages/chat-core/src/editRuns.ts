import type { TimelineItem, ToolEvent } from "./toolEvents";
import type { FileDiff } from "./types";

export function isTurnDiff(event: ToolEvent): boolean {
  return event.kind === "edit" && event.phase === "call" && !event.toolId
    && event.message.toolName === "turn_diff";
}

const WINDOWS_ROOT = /^(?:[a-z]:\/|\/\/[^/]+\/[^/]+(?:\/|$))/i;

function windowsSeparators(path: string): string {
  return path.replace(/\\/g, "/").replace(/^\/\/\?\/UNC\//i, "//").replace(/^\/\/\?\//, "");
}

function comparablePath(path: string, cwd?: string | null): string {
  const windowsPath = windowsSeparators(path);
  const windowsCwd = cwd ? windowsSeparators(cwd) : "";
  const windows = WINDOWS_ROOT.test(windowsPath) || WINDOWS_ROOT.test(windowsCwd);
  const base = windows ? windowsCwd : cwd ?? "";
  const normalized = windows || (!cwd && !path.startsWith("/")) ? windowsPath : path;
  const absolute = normalized.startsWith("/") || WINDOWS_ROOT.test(normalized);
  const absoluteBase = base.startsWith("/") || WINDOWS_ROOT.test(base);
  const resolved = absolute || !absoluteBase || (windows && /^[a-z]:/i.test(normalized))
    ? normalized : `${base.replace(/\/+$/, "")}/${normalized}`;
  const root = WINDOWS_ROOT.exec(resolved)?.[0].replace(/\/$/, "")
    ?? (resolved.startsWith("/") ? "" : undefined);
  if (root === undefined) return windows ? resolved.toLowerCase() : resolved;
  const segments: string[] = [];
  for (const segment of resolved.slice(root.length).split("/")) {
    if (segment === "..") segments.pop();
    else if (segment && segment !== ".") segments.push(segment);
  }
  const identity = `${root}/${segments.join("/")}`;
  return windows ? identity.toLowerCase() : identity;
}

function changeKey(event: ToolEvent, diff: FileDiff, cwd?: string | null): string {
  return JSON.stringify([
    event.message.sourceSessionId ?? "", event.flowId, comparablePath(diff.path, cwd),
    diff.movePath === undefined ? undefined : comparablePath(diff.movePath, cwd), diff.kind ?? "modify",
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
export function reconcileTurnDiffs(items: TimelineItem[], cwd?: string | null): Set<number> {
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
      const key = changeKey(event, diff, cwd);
      const changes = recorded.get(key);
      if (changes) changes.push(diff);
      else recorded.set(key, [diff]);
    }
  }
  for (const [index, event] of snapshots) {
    const diffs = event.fileDiffs ?? [];
    const remaining = diffs.filter((diff) => !recorded.get(changeKey(event, diff, cwd))?.some((change) => sameChange(change, diff)));
    if (!remaining.length) suppressed.add(index);
    else if (remaining.length !== diffs.length) {
      items[index] = { kind: "tool", event: { ...event, fileDiffs: remaining, summary: remaining.map((diff) => diff.path).join("\n") } };
    }
  }
  return suppressed;
}

/** A standalone structured edit that can sit in a run; streamed, batched and grouped events keep their own card. */
function runKey(event: ToolEvent, liveTools: boolean): string | undefined {
  if (event.kind !== "edit" || event.phase !== "call") return undefined;
  if (event.stream || event.commandBatch || event.agents || event.editRun) return undefined;
  if (liveTools && event.message.toolStatus && !isTurnDiff(event)) return `live:${event.message.sourceSessionId ?? ""}`;
  if (!event.fileDiffs?.length) return undefined;
  return `recorded:${event.message.sourceSessionId ?? ""}`;
}

function runEvent(members: ToolEvent[], live: boolean): ToolEvent {
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
    ...(live ? { liveEditRun: true } : {}),
    status,
    result: undefined,
    error: members.some((member) => member.status === "error") ? true : undefined,
  };
}

/** Folds back-to-back file edits of one session into a single run, so a burst of edits reads as one tree. */
export function groupEditRuns(items: TimelineItem[], liveTools = false): TimelineItem[] {
  const result: TimelineItem[] = [];
  let members: ToolEvent[] = [];
  let key: string | undefined;
  const flush = () => {
    if (members.length) {
      const live = key?.startsWith("live:") === true;
      result.push({ kind: "tool", event: live || members.length > 1 ? runEvent(members, live) : members[0] });
    }
    members = [];
    key = undefined;
  };
  for (const item of items) {
    const nextKey = item.kind === "tool" ? runKey(item.event, liveTools) : undefined;
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
