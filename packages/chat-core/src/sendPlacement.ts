import { sendRow, type ChatSend } from "./sendOrder";
import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

export function placePendingSends(
  committed: readonly NormalizedChatMessage[],
  pending: readonly ChatSend[],
): NormalizedChatMessage[] {
  const before = new Map<number, NormalizedChatMessage[]>();
  const represented = committed.filter(row => row.inheritedFromSessionId == null && (row.type === "user" || row.type === "local_command"));
  const representedIds = new Set(represented.map(row => row.id));
  const representedTokens = new Set(represented.flatMap(row => row.correlationId ? [row.correlationId] : []));
  for (const send of pending) {
    if (representedIds.has(send.id) || (send.correlationId && representedTokens.has(send.correlationId))) continue;
    const flowAtMs = send.held ? undefined : send.flowAtMs;
    const index = flowAtMs == null ? -1 : committed.findIndex((row) => {
      const at = timestampMs(row.timestamp);
      return at != null && at > flowAtMs;
    });
    const position = index < 0 ? committed.length : index;
    const rows = before.get(position) ?? [];
    rows.push(sendRow(send));
    before.set(position, rows);
  }
  return committed.flatMap((row, index) => [...(before.get(index) ?? []), row])
    .concat(before.get(committed.length) ?? []);
}
