import type { NormalizedChatMessage } from "./types";

/** Known IDs keep their slot; a new row lands before the next incoming row already merged. */
export function mergeById<T extends NormalizedChatMessage>(prev: T[], incoming: T[]): T[] {
  const latestIncoming = new Map(incoming.map(message => [message.id, message]));
  const orderedIncoming = [...latestIncoming.values()].sort((left, right) => {
    if (left.seq != null && right.seq != null) return left.seq - right.seq;
    return 0;
  });
  const known = new Map<string, T>();
  for (const message of prev) known.set(message.id, message);
  const before = new Map<string, T[]>();
  let fresh: T[] = [];
  for (const message of orderedIncoming) {
    if (!known.has(message.id)) {
      fresh.push(message);
      continue;
    }
    if (fresh.length > 0) before.set(message.id, fresh);
    fresh = [];
  }
  const entries: T[] = [];
  const placed = new Set<string>();
  for (const message of prev) {
    if (placed.has(message.id)) continue;
    placed.add(message.id);
    entries.push(...(before.get(message.id) ?? []), latestIncoming.get(message.id) ?? known.get(message.id)!);
  }
  return entries.concat(fresh);
}
