import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

function mergeIdentity(message: NormalizedChatMessage): string {
  if (message.type === "user" && message.seq != null) {
    return `user-seq:${message.sourceSessionId ?? "unbound"}:${message.seq}`;
  }
  return `id:${message.id}`;
}

/** Merge a freshly-arrived batch into the existing transcript: dedupe by stable
 *  `id` (incoming wins, so a re-emitted message updates in place) then order by
 *  `(timestamp, seq, arrival)`.
 *
 *  The ordering contract is provider-agnostic. Two signals can be missing or
 *  coarse, so neither alone is trusted:
 *    - `timestamp` — codex floors to whole seconds and emits ISO strings; cursor
 *      emits none at all. So we only let time decide when BOTH sides parse to a
 *      finite epoch-ms AND differ (a raw `Number("2026-..ISO..")` is NaN — the
 *      old comparator's silent no-op that let same-second turns shuffle).
 *    - `seq` — the server's monotonic source ordinal (JSONL rich-message index /
 *      structured row position). It is the stable tiebreaker for equal/missing
 *      timestamps, falling back to arrival order when a provider supplies none. */
export function mergeById<T extends NormalizedChatMessage>(prev: T[], incoming: T[]): T[] {
  const map = new Map<string, T>();
  for (const m of prev) map.set(mergeIdentity(m), m);
  for (const m of incoming) map.set(mergeIdentity(m), m);
  return Array.from(map.values())
    .map((message, arrival) => ({ message, arrival }))
    .sort((a, b) => {
      const at = timestampMs(a.message.timestamp);
      const bt = timestampMs(b.message.timestamp);
      if (at != null && bt != null && at !== bt) return at - bt;
      const as = a.message.seq;
      const bs = b.message.seq;
      if (as != null && bs != null && as !== bs) return as - bs;
      return a.arrival - b.arrival;
    })
    .map((entry) => entry.message);
}
