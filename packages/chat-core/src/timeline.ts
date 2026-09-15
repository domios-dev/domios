import type { NormalizedChatMessage } from "./types";

/** Rows flattened from one source record share its id base inside one batch; a base can
 *  be a reused stream ordinal, so it never links rows across batches. */
function sourceEntry(row: NormalizedChatMessage): string | null {
  if (row.sourceMessageId == null) return null;
  const separator = row.id.indexOf(":");
  const batch = separator < 0 ? "" : row.id.slice(0, separator);
  return `${batch}|${row.sourceMessageId.replace(/-\d+$/, "")}`;
}

/** Keys are epoch ms of a row's own event, null when neither the row nor a proven parent has one.
 *  A row inherits only from its event parent: a result from its call, a block from its record. */
export function inheritedOrderKeys(
  rows: readonly NormalizedChatMessage[],
  ownKey: (row: NormalizedChatMessage, index: number) => number | null,
): Array<number | null> {
  const callKeys = new Map<string, number>();
  const entryKeys = new Map<string, number>();
  return rows.map((row, index) => {
    const entry = sourceEntry(row);
    let key = ownKey(row, index);
    if (key == null && row.type === "tool_result" && row.toolId) key = callKeys.get(row.toolId) ?? null;
    if (key == null && entry != null) key = entryKeys.get(entry) ?? null;
    if (key == null) return null;
    if (row.type === "tool_call" && row.toolId && !callKeys.has(row.toolId)) callKeys.set(row.toolId, key);
    if (entry != null && !entryKeys.has(entry)) entryKeys.set(entry, key);
    return key;
  });
}

/** The one total order: keyed rows sort by key (source position breaks ties) into the
 *  positions keyed rows occupy; keyless rows hold their position and block nothing. */
export function chronologicalOrder<T>(rows: T[], keys: ReadonlyArray<number | null>): T[] {
  const slots: number[] = [];
  keys.forEach((key, index) => {
    if (key != null) slots.push(index);
  });
  const sorted = [...slots].sort((a, b) => (keys[a] as number) - (keys[b] as number) || a - b);
  if (sorted.every((index, position) => index === slots[position])) return rows;
  const out = [...rows];
  sorted.forEach((index, position) => {
    out[slots[position]] = rows[index];
  });
  return out;
}
