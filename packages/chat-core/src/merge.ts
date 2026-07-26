import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

function mergeIdentities(message: NormalizedChatMessage): string[] {
  const identities = [`id:${message.id}`];
  if ((message.type === "user" || message.type === "assistant") && message.seq != null) {
    identities.push(`${message.type}-seq:${message.sourceSessionId ?? "unbound"}:${message.seq}`);
  }
  return identities;
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
  const aliases = new Map<string, number>();
  const entries: Array<{ message: T; arrival: number } | null> = [];
  for (const message of [...prev, ...incoming]) {
    const identities = mergeIdentities(message);
    const matchedIndexes = [
      ...new Set(identities.map((identity) => aliases.get(identity)).filter((index) => index != null)),
    ];
    const index = matchedIndexes[0] ?? entries.length;
    const arrival = entries[index]?.arrival ?? index;
    entries[index] = { message, arrival };
    for (const matchedIndex of matchedIndexes.slice(1)) {
      const matched = entries[matchedIndex];
      if (matched) {
        for (const identity of mergeIdentities(matched.message)) aliases.set(identity, index);
        entries[matchedIndex] = null;
      }
    }
    for (const identity of identities) aliases.set(identity, index);
  }
  return entries
    .filter((entry): entry is { message: T; arrival: number } => entry != null)
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
