import { formatUntil } from "../../lib/timeLabels";
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

/** Why a message of yours did not reach them, in plain words, with when to try again when that is known. */
export function refusalLine(entry: HistoryEntry, friendName: string, now: number): string | null {
  const retry = entry.retry_after_seconds !== undefined ? ` Try again ${formatUntil(now + entry.retry_after_seconds * 1000, now)}.` : "";
  switch (entry.reason) {
    case "neighbor_daily_limit":
      return `Limit reached: ${friendName} allows ${entry.limit ?? "a few"} messages a day; ask them to raise it.${entry.resets_at ? ` Resets ${formatUntil(entry.resets_at, now)}.` : ""}`;
    case "rate_limited": return `Not delivered: ${friendName} is getting too many messages right now.${retry}`;
    case "aggregate_limited": return `Not delivered: you have sent too many Neighbor messages for now.${retry}`;
    case "friends_locked": return "Not delivered: your plan pauses Neighbor messages.";
    case "recipient_locked": return `Not delivered: ${friendName}'s plan pauses their Neighbor messages.`;
    case "recipient_not_receiving": return `Not delivered: ${friendName} is not receiving right now.`;
    default: return null;
  }
}

/** Who a message is from, as the reader sees it: you, your agent, your friend, or your friend's concierge. */
export type AuthorKind = "me" | "my-agent" | "friend" | "friend-agent";

export function authorKind(entry: HistoryEntry): AuthorKind {
  if (entry.direction === "out") return entry.author === "me" ? "me" : "my-agent";
  return entry.kind === "answer" || entry.kind === "refusal" ? "friend-agent" : "friend";
}

/** A message continues its author's group when the same author spoke last on the same day. */
export function continuesGroup(entry: HistoryEntry, previous: HistoryEntry | null): boolean {
  if (!previous || authorKind(entry) !== authorKind(previous)) return false;
  if (new Date(entry.at).toDateString() !== new Date(previous.at).toDateString()) return false;
  const name = (value: HistoryEntry) => typeof value.author === "object" ? value.author.agent : value.author;
  return name(entry) === name(previous);
}
