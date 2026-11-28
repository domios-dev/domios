import type { NormalizedChatMessage } from "./types";
import { retainRoutedPresentation } from "./routedPresentation";

function uniqueRows<T extends NormalizedChatMessage>(rows: readonly T[]): T[] {
  const known = new Map<string, T>();
  for (const row of rows) {
    const previous = known.get(row.id);
    known.set(row.id, previous ? retainRoutedPresentation(previous, row) : row);
  }
  return [...known.values()];
}

/** Known IDs keep their slot; a new row lands before the next incoming row already merged. */
export function mergeById<T extends NormalizedChatMessage>(prev: T[], incoming: T[]): T[] {
  const ordered = uniqueRows(incoming)
    .sort((left, right) => left.seq != null && right.seq != null ? left.seq - right.seq : 0);
  return mergeBatchesById([prev, ordered]);
}

/** Source segments already carry order; cached segments can contain restarted local ordinals. */
export function mergeBatchesById<T extends NormalizedChatMessage>(batches: Iterable<readonly T[]>): T[] {
  interface Entry { value: T; previous: Entry | null; next: Entry | null }
  const known = new Map<string, Entry>();
  let first: Entry | null = null;
  let last: Entry | null = null;
  for (const batch of batches) {
    const incoming = uniqueRows(batch);
    let anchor: Entry | null = null;
    for (let index = incoming.length - 1; index >= 0; index--) {
      const value = incoming[index];
      const existing = known.get(value.id);
      if (existing) {
        const updated = retainRoutedPresentation(existing.value, value);
        existing.value = updated.timestamp == null && existing.value.timestamp != null
          ? { ...updated, timestamp: existing.value.timestamp }
          : updated;
        anchor = existing;
        continue;
      }
      const entry: Entry = { value, previous: anchor ? anchor.previous : last, next: anchor };
      if (entry.previous) entry.previous.next = entry;
      else first = entry;
      if (anchor) anchor.previous = entry;
      else last = entry;
      known.set(value.id, entry);
      anchor = entry;
    }
  }
  const output: T[] = [];
  for (let entry = first; entry; entry = entry.next) output.push(entry.value);
  return output;
}
