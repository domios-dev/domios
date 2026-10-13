import type { NormalizedChatMessage } from "./types";

export function mergeById<T extends NormalizedChatMessage>(prev: T[], incoming: T[]): T[] {
  const slots = new Map<string, number>();
  const entries: Array<{ message: T; arrival: number }> = [];
  for (const message of [...prev, ...incoming]) {
    const slot = slots.get(message.id);
    if (slot != null) {
      entries[slot] = { message, arrival: entries[slot].arrival };
      continue;
    }
    slots.set(message.id, entries.length);
    entries.push({ message, arrival: entries.length });
  }
  return entries
    .sort((a, b) => {
      const as = a.message.seq;
      const bs = b.message.seq;
      if (as != null && bs != null && as !== bs) return as - bs;
      return a.arrival - b.arrival;
    })
    .map((entry) => entry.message);
}
