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
      const at = timestampMs(a.message.timestamp);
      const bt = timestampMs(b.message.timestamp);
      if (at != null && bt != null && at !== bt) return at - bt;
      // Equal/missing timestamps: confirmed turns carry the server's source-order
      // `seq` (codex same-second, cursor timeless); fall back to arrival index so
      // seq-less optimistic bubbles keep their appended position.
      const as = a.message.seq;
      const bs = b.message.seq;
      if (as != null && bs != null && as !== bs) return as - bs;
      return a.index - b.index;
    })
    .map((entry) => entry.message);
}
