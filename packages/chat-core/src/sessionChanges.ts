import { wellFormedFileDiffs } from "./fileDiffs";
import type { FileDiff, NormalizedChatMessage, ToolEdit } from "./types";

export interface RecordedFileChange {
  path: string;
  movePath?: string;
  kind: "add" | "modify" | "delete" | "rename";
  additions: number;
  deletions: number;
  diffs: FileDiff[];
  turnIds: string[];
  binary: boolean;
  truncated: boolean;
}

export interface ChangesTurn {
  turnId: string;
  at?: string;
  files: RecordedFileChange[];
}

export interface SessionChanges {
  turns: ChangesTurn[];
  cumulative: RecordedFileChange[];
}

function lines(text: string): string[] {
  return text ? text.replace(/\r\n/g, "\n").replace(/\n$/, "").split("\n") : [];
}

export function fileDiffFromEdit(edit: ToolEdit): FileDiff {
  const old = lines(edit.old);
  const next = lines(edit.new);
  return {
    path: edit.path, additions: next.length, deletions: old.length,
    hunks: [{ oldStart: 1, oldLines: old.length, newStart: 1, newLines: next.length,
      lines: [...old.map((line) => `-${line}`), ...next.map((line) => `+${line}`)] }],
  };
}

function fold(files: RecordedFileChange[], diff: FileDiff, turnId: string): void {
  const previous = files.find((file) => (file.movePath ?? file.path).replace(/\\/g, "/") === diff.path.replace(/\\/g, "/"));
  const truncated = Boolean(diff.truncated) || diff.hunks.some((hunk) =>
    hunk.lines.filter((line) => /^[ -]/.test(line)).length !== hunk.oldLines
    || hunk.lines.filter((line) => /^[ +]/.test(line)).length !== hunk.newLines);
  if (previous) {
    previous.movePath = diff.movePath ?? previous.movePath;
    previous.kind = diff.kind === "delete" ? "delete" : previous.kind === "add" ? "add"
      : previous.movePath ? "rename" : diff.kind === "add" && previous.kind === "delete" ? "modify" : previous.kind;
    previous.additions += diff.additions;
    previous.deletions += diff.deletions;
    previous.diffs.push(diff);
    previous.binary ||= Boolean(diff.binary);
    previous.truncated ||= truncated;
    if (!previous.turnIds.includes(turnId)) previous.turnIds.push(turnId);
    return;
  }
  files.push({ path: diff.path, movePath: diff.movePath,
    kind: diff.movePath ? "rename" : diff.kind ?? "modify",
    additions: diff.additions, deletions: diff.deletions, diffs: [diff], turnIds: [turnId],
    binary: Boolean(diff.binary), truncated });
}

export function sessionChanges(messages: readonly NormalizedChatMessage[]): SessionChanges {
  const turns: ChangesTurn[] = [];
  const cumulative: RecordedFileChange[] = [];
  let turn: ChangesTurn | undefined;
  const calls = new Map<string, { message: NormalizedChatMessage; turn: ChangesTurn }>();
  const records: Array<{ message: NormalizedChatMessage; turn: ChangesTurn }> = [];
  const seen = new Set<string>();
  for (const message of messages) {
    const identity = `${message.sourceSessionId ?? ""}:${message.id}`;
    if (seen.has(identity)) continue;
    seen.add(identity);
    if (message.type === "user") {
      turn = { turnId: message.sourceMessageId ?? message.id, at: message.timestamp, files: [] };
      turns.push(turn);
    }
    const key = message.toolId ? `${message.sourceSessionId ?? ""}:${message.toolId}` : undefined;
    const existing = key ? calls.get(key) : undefined;
    if (!existing && !message.fileDiffs?.length && !message.toolEdits?.length) continue;
    if (!turn) {
      turn = { turnId: message.sourceMessageId ?? message.id, at: message.timestamp, files: [] };
      turns.push(turn);
    }
    if (existing) {
      existing.message = { ...existing.message, ...message,
        fileDiffs: message.fileDiffs?.length ? message.fileDiffs : existing.message.fileDiffs,
        toolEdits: message.toolEdits?.length ? message.toolEdits : existing.message.toolEdits };
    } else {
      const record = { message, turn };
      records.push(record);
      if (key) calls.set(key, record);
    }
  }
  for (const { message, turn: owner } of records) {
    if (message.toolError || message.toolInterrupted || (message.toolExitCode ?? 0) !== 0) continue;
    if (!message.fileDiffs?.length && message.type !== "tool_result" && message.toolError !== false && message.toolResult === undefined) continue;
    const structured = wellFormedFileDiffs(message.fileDiffs) ?? [];
    const covered = new Set(structured.flatMap((diff) => [diff.path, diff.movePath].filter((path): path is string => Boolean(path))));
    const diffs = [...structured, ...(message.toolEdits ?? [])
      .filter((edit) => edit.path && edit.old !== edit.new && !covered.has(edit.path)).map(fileDiffFromEdit)];
    for (const diff of diffs) {
      fold(owner.files, diff, owner.turnId);
      fold(cumulative, diff, owner.turnId);
    }
  }
  return { turns, cumulative };
}
