import { timestampMs, type NormalizedChatMessage } from "@codeterm/chat-core";
import type { TabAgentState } from "../../lib/types";

export function compactionPhase(live?: TabAgentState | null, failed = false): "compacting" | "other" | "failed" | "unknown" {
  return failed || live?.phase === "failed" || live?.subtype === "error" ? "failed"
    : !live ? "unknown" : live.state === "working" && live.phase === "compacting" ? "compacting" : "other";
}

export interface ContextCompaction {
  id: string;
  afterId: string | null;
  startedAt: number;
  endedAt?: number;
  state: "running" | "completed" | "failed";
  providerRowId?: string;
}

export interface CompactionHistory {
  compacting: boolean;
  rows: readonly ContextCompaction[];
}

export const emptyCompactionHistory: CompactionHistory = { compacting: false, rows: [] };

export function observeCompaction(
  history: CompactionHistory,
  input: { phase: "compacting" | "other" | "failed" | "unknown"; at: number; id: string; afterId: string | null },
): CompactionHistory {
  if (input.phase === "unknown") return history;
  const compacting = input.phase === "compacting";
  if (compacting === history.compacting && input.phase !== "failed") {
    const tail = history.rows[history.rows.length - 1];
    if (!compacting || tail?.state !== "running" || tail.afterId === input.afterId) return history;
    return { ...history, rows: [...history.rows.slice(0, -1), { ...tail, afterId: input.afterId }] };
  }
  if (compacting) {
    return { compacting, rows: [...history.rows, {
      id: input.id, afterId: input.afterId, startedAt: input.at, state: "running",
    }] };
  }
  let active = -1;
  for (let index = history.rows.length - 1; index >= 0; index--) {
    if (history.rows[index].state === "running") { active = index; break; }
  }
  if (active < 0 && history.compacting && input.phase === "failed") active = history.rows.length - 1;
  if (active < 0) return history.compacting ? { ...history, compacting } : history;
  return { compacting, rows: history.rows.map((row, index) => index === active
    ? { ...row, afterId: input.afterId ?? row.afterId, endedAt: input.at, state: input.phase === "failed" ? "failed" : "completed" } : row) };
}

export function reconcileCompactionRows(history: CompactionHistory, messages: readonly NormalizedChatMessage[], at: number): CompactionHistory {
  const claimed = new Set(history.rows.flatMap((row) => row.providerRowId ? [row.providerRowId] : []));
  let changed = false;
  const rows = history.rows.map((row, rowIndex) => {
    if (row.providerRowId || row.state === "failed") return row;
    const anchor = row.afterId == null ? -1 : messages.findIndex((message) => message.id === row.afterId);
    const provider = messages.find((message, index) => message.timelineMarker?.kind === "context_compaction"
      && !claimed.has(message.id)
      && ((timestampMs(message.timestamp) ?? Infinity) <= (row.endedAt ?? Infinity)
        || (timestampMs(message.timestamp) == null && rowIndex === history.rows.length - 1))
      && ((anchor >= 0 && index > anchor)
        || (timestampMs(message.timestamp) ?? -1) >= row.startedAt));
    if (!provider) return row;
    claimed.add(provider.id);
    changed = true;
    return { ...row, state: "completed" as const, endedAt: row.endedAt ?? at, providerRowId: provider.id };
  });
  return changed ? { ...history, rows } : history;
}

export function projectCompactionRows(messages: readonly NormalizedChatMessage[], history: CompactionHistory): NormalizedChatMessage[] {
  const replaced = new Set(history.rows.flatMap((row) => row.providerRowId ? [row.providerRowId] : []));
  const result = messages.filter((message) => !replaced.has(message.id));
  for (const row of history.rows) {
    if (result.some((message) => message.id === row.id)) continue;
    const anchor = row.afterId == null ? -1 : result.findIndex((message) => message.id === row.afterId);
    const provider = row.providerRowId ? messages.find((message) => message.id === row.providerRowId) : undefined;
    let index = row.state === "running" ? result.length : anchor >= 0 ? anchor + 1
      : result.findIndex((message) => (timestampMs(message.timestamp) ?? -1) >= row.startedAt);
    if (index < 0) {
      const last = result[result.length - 1];
      if (last && (timestampMs(last.timestamp) ?? Infinity) < row.startedAt && row.afterId != null) continue;
      index = result.length;
    }
    while (index < result.length && result[index].contextCompaction
      && result[index].contextCompaction!.startedAt <= row.startedAt) index += 1;
    result.splice(index, 0, {
      id: row.id, type: "system", content: "", timestamp: String(row.startedAt),
      timelineMarker: { kind: "context_compaction" }, contextCompaction: row,
      sourceSessionId: provider?.sourceSessionId,
    });
  }
  return result;
}
