import type { NormalizedChatMessage } from "./types";

export function mergeById<T extends NormalizedChatMessage>(prev: T[], incoming: T[]): T[] {
  const slots = new Map<string, number>();
  const entries: T[] = [];
  const latestIncoming = new Map(incoming.map(message => [message.id, message]));
  const orderedIncoming = [...latestIncoming.values()].sort((left, right) => {
    if (left.seq != null && right.seq != null) return left.seq - right.seq;
    return 0;
  });
  for (const message of [...prev, ...orderedIncoming]) {
    const slot = slots.get(message.id);
    if (slot != null) {
      entries[slot] = message;
      continue;
    }
    slots.set(message.id, entries.length);
    entries.push(message);
  }
  return entries;
}
