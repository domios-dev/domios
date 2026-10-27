import type { HistoryEntry } from "./neighborConciergeClient";

/** Pages arrive newest first; the conversation reads oldest first, one copy per message id. */
export function mergeHistory(current: readonly HistoryEntry[], page: readonly HistoryEntry[]): HistoryEntry[] {
  const byId = new Map(current.map(entry => [entry.message_id, entry]));
  for (const entry of page) byId.set(entry.message_id, entry);
  return [...byId.values()].sort((a, b) => a.at - b.at || a.message_id.localeCompare(b.message_id));
}

export function speaker(entry: HistoryEntry, friendName: string): string {
  if (entry.author === "me") return "You";
  if (entry.author === "friend") return friendName;
  return entry.author.name;
}

/** A calendar day label for the first message of each day, in the viewer's locale. */
export function dayLabel(at: number, previous: number | null, locale?: string): string | null {
  const day = new Date(at).toDateString();
  if (previous !== null && new Date(previous).toDateString() === day) return null;
  return new Date(at).toLocaleDateString(locale, { weekday: "short", month: "short", day: "numeric" });
}

export const DELIVERY_LABEL = { pending: "sending", delivered: "delivered", not_delivered: "not delivered" } as const;
