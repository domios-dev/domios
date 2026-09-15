import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

export function mergeTimelineMessages<T extends NormalizedChatMessage>(
  confirmed: T[],
  pending: T[],
): T[] {
  if (pending.length === 0) return confirmed;
  return [
    ...confirmed.map((message, index) => ({ message, index })),
    ...pending.map((message, index) => ({ message, index: confirmed.length + index })),
  ]
    .sort((a, b) => {
      const ao = a.message.clientOrder;
      const bo = b.message.clientOrder;
      if (ao != null && bo != null && ao !== bo) return ao - bo;
      const at = timestampMs(a.message.timestamp);
      const bt = timestampMs(b.message.timestamp);
      if (at != null && bt != null && at !== bt) return at - bt;
      const as = a.message.seq;
      const bs = b.message.seq;
      if (as != null && bs != null && as !== bs) return as - bs;
      return a.index - b.index;
    })
    .map((entry) => entry.message);
}

function isUserAuthored(message: NormalizedChatMessage): boolean {
  return message.type === "user" || message.type === "local_command";
}

function ownTimestampSlot(
  rows: readonly NormalizedChatMessage[],
  end: number,
  ts: number | null,
): number {
  if (ts == null) return end;
  let slot = end;
  while (slot > 0) {
    const prior = rows[slot - 1];
    const priorTs = timestampMs(prior.timestamp);
    if (isUserAuthored(prior) || priorTs == null || priorTs <= ts) break;
    slot -= 1;
  }
  return slot;
}

/** A source records input sent mid-turn where the agent consumed it but stamps it
 *  at send time, so a user row takes its own timestamp slot among later output. */
export function chronologicalRows<T extends NormalizedChatMessage>(rows: T[]): T[] {
  let out: T[] | null = null;
  for (let index = 0; index < rows.length; index += 1) {
    const row = rows[index];
    const placed: readonly T[] = out ?? rows;
    const end = out ? out.length : index;
    const slot = row.type === "user" ? ownTimestampSlot(placed, end, timestampMs(row.timestamp)) : end;
    if (out == null && slot !== end) out = rows.slice(0, index);
    if (out != null) out.splice(slot, 0, row);
  }
  return out ?? rows;
}
