import { sendRow, type ChatSend } from "./sendOrder";
import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

export function placePendingSends(
  committed: readonly NormalizedChatMessage[],
  pending: readonly ChatSend[],
): NormalizedChatMessage[] {
  const before = new Map<number, NormalizedChatMessage[]>();
  for (const send of pending) {
    const flowAtMs = send.flowAtMs;
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
