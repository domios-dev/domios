import type { NormalizedChatMessage } from "./types";

/** `seq` is a per-batch ordinal that other batches reuse for unrelated rows,
 *  so only the provider id identifies a row here. */
function mergeIdentities(message: NormalizedChatMessage): string[] {
  return [`id:${message.id}`];
}

export function mergeById<T extends NormalizedChatMessage>(prev: T[], incoming: T[]): T[] {
  const slots = new Map<string, number>();
  const entries: Array<{ message: T; arrival: number }> = [];
  for (const message of [...prev, ...incoming]) {
    const [identity] = mergeIdentities(message);
    const slot = slots.get(identity);
    if (slot != null) {
      entries[slot] = { message, arrival: entries[slot].arrival };
      continue;
    }
    slots.set(identity, entries.length);
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
