import { claimedRecordLoaded, sendRow, type ChatSend } from "./sendOrder";
import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

export function pendingSendPosition(committed: readonly NormalizedChatMessage[], send: ChatSend): number {
  const record = send.matchedRow && claimedRecordLoaded(committed, send.matchedRow) ? send.matchedRow.source.record : undefined;
  const flowAtMs = send.held ? undefined : send.flowAtMs;
  const index = record?.kind === "jsonl" ? committed.findIndex(row => {
    const source = row.sourceRecord;
    return row.sourceSessionId === send.matchedRow!.source.source_session_id
      && source?.kind === "jsonl" && source.incarnation === record.incarnation
      && (source.record_start > record.record_start
        || (source.record_start === record.record_start && source.record_row > record.record_row));
  }) : flowAtMs == null ? -1 : committed.findIndex(row => {
    if (record?.kind === "structured" && (row.sourceSessionId !== record.thread_id || row.inheritedFromSessionId != null)) return false;
    const at = timestampMs(row.timestamp);
    return at != null && at > flowAtMs;
  });
  return index < 0 ? committed.length : index;
}

export function placePendingSends(
  committed: readonly NormalizedChatMessage[],
  pending: readonly ChatSend[],
): NormalizedChatMessage[] {
  const before = new Map<number, NormalizedChatMessage[]>();
  const represented = committed.filter(row => row.inheritedFromSessionId == null && (row.type === "user" || row.type === "local_command"));
  const representedIds = new Set(represented.map(row => row.id));
  const representedTokens = new Set(represented.flatMap(row => row.correlationId ? [row.correlationId] : []));
  const ordered = [...pending].sort((left, right) => left.sentAtMs - right.sentAtMs);
  const groups = new Map<string, number[]>();
  ordered.forEach((send, index) => {
    const source = send.matchedRow?.source;
    if (source?.record.kind !== "jsonl") return;
    const key = JSON.stringify([source.source_session_id, source.record.incarnation]);
    const positions = groups.get(key) ?? [];
    positions.push(index);
    groups.set(key, positions);
  });
  for (const positions of groups.values()) {
    const sends = positions.map(index => ordered[index]).sort((left, right) => {
      const a = left.matchedRow!.source.record;
      const b = right.matchedRow!.source.record;
      return a.kind === "jsonl" && b.kind === "jsonl"
        ? a.record_start - b.record_start || a.record_row - b.record_row || left.matchedRow!.ordinal - right.matchedRow!.ordinal : 0;
    });
    positions.forEach((position, index) => { ordered[position] = sends[index]; });
  }
  for (const send of ordered) {
    if (representedIds.has(send.id) || (send.correlationId && representedTokens.has(send.correlationId))) continue;
    const position = pendingSendPosition(committed, send);
    const rows = before.get(position) ?? [];
    rows.push(sendRow(send));
    before.set(position, rows);
  }
  return committed.flatMap((row, index) => [...(before.get(index) ?? []), row])
    .concat(before.get(committed.length) ?? []);
}
