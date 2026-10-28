import { formatUntil } from "../../lib/timeLabels";
import type { HandlingState, HistoryEntry } from "./neighborConciergeClient";

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

/** Where your own message stands, as messengers mark it: sending until the relay took it, then sent, delivered, read. */
export type ReceiptMark = "sending" | "sent" | "delivered" | "read" | "not_delivered";

export function receiptMark(entry: HistoryEntry): ReceiptMark | null {
  if (entry.direction !== "out" || !entry.delivery_status) return null;
  if (entry.delivery_status === "not_delivered") return "not_delivered";
  if (entry.read_at !== undefined) return "read";
  if (entry.delivery_status === "delivered") return "delivered";
  return entry.submission === "accepted" ? "sent" : "sending";
}

/** A message you sent that history has not shown yet: its bubble appears at once and becomes the history entry when it lands.
 *  An uncertain send has no receipt, so it is never sent again; it waits to be recognised in history by its words. */
export interface OutgoingSend {
  key: string;
  body: string;
  at: number;
  state: "sending" | "failed";
  messageId?: string;
  uncertain?: boolean;
  reason?: string;
}

/** Which sends history now holds, as send key to message id, each history entry claimed once. */
export function landedSends(sends: readonly OutgoingSend[], entries: readonly HistoryEntry[], claimed: ReadonlySet<string> = new Set()): Map<string, string> {
  const landed = new Map<string, string>();
  const taken = new Set(claimed);
  for (const send of sends) {
    const entry = send.messageId ? entries.find(item => item.message_id === send.messageId)
      : send.uncertain ? entries.find(item => item.direction === "out" && item.author === "me" && item.body === send.body
        && item.at >= send.at - 2000 && !taken.has(item.message_id)) : undefined;
    if (!entry) continue;
    taken.add(entry.message_id);
    landed.set(send.key, entry.message_id);
  }
  return landed;
}

/** Why a message of yours did not reach them, in plain words, with when to try again when that is known. */
export function refusalLine(entry: HistoryEntry, friendName: string, now: number): string | null {
  const retry = entry.retry_after_seconds !== undefined ? ` Try again ${formatUntil(now + entry.retry_after_seconds * 1000, now)}.` : "";
  switch (entry.reason) {
    case "neighbor_daily_limit":
      return `Limit reached: ${friendName} allows ${entry.limit ?? "a few"} messages a day; ask them to raise it.${entry.resets_at ? ` Resets ${formatUntil(entry.resets_at, now)}.` : ""}`;
    case "rate_limited": return `Not delivered: ${friendName} is getting too many messages right now.${retry}`;
    case "aggregate_limited": return `Not delivered: you have sent too many Neighbor messages for now.${retry}`;
    case "aggregate_daily_limited":
      return `You reached today's sending limit${entry.limit !== undefined ? ` (${entry.limit})` : ""}.${entry.resets_at ? ` It resets ${formatUntil(entry.resets_at, now)}.` : ""}`;
    case "friends_locked": return "Not delivered: your plan pauses Neighbor messages.";
    case "recipient_locked": return `Not delivered: ${friendName}'s plan pauses their Neighbor messages.`;
    case "recipient_not_receiving": return `Not delivered: ${friendName} is not receiving right now.`;
    case "recipient_paused": return `Not delivered: ${friendName} disabled messages from you.`;
    default: return null;
  }
}

/** Who a message is from, as the reader sees it: you, your agent, your friend, or your friend's concierge. */
export type AuthorKind = "me" | "my-agent" | "friend" | "friend-agent";

export function authorKind(entry: HistoryEntry): AuthorKind {
  if (entry.direction === "out") return entry.author === "me" ? "me" : "my-agent";
  return entry.kind === "answer" || entry.kind === "refusal" ? "friend-agent" : "friend";
}

/** Where your concierge stands on a message from your friend. */
export type ConciergeWork = HandlingState;

export const CONCIERGE_WORK_LINE: Record<ConciergeWork, string> = {
  delivered_to_concierge: "With your concierge", working: "Your concierge is working", answered: "Answered", refused: "Declined",
  no_reply: "No reply needed", failed: "Couldn't reach your concierge", waiting: "Waiting for a concierge on this machine", stopped: "Stopped",
};

/** States your concierge is still busy with; Stop applies to them. */
export const activeWork = (work: ConciergeWork) => work === "delivered_to_concierge" || work === "working";

export interface ConciergeWorkEntry { work: ConciergeWork; reply?: string; reason?: string }

/** Each friend message's standing from the runtime's handling state, with the reply it links to.
 *  Before a runtime reports handling, the concierge tab's activity marks only the newest open message. */
export function conciergeWork(entries: readonly HistoryEntry[], concierge: { working: boolean; missing: boolean }): Map<string, ConciergeWorkEntry> {
  const replies = new Map<string, string>();
  for (const entry of entries) if (entry.direction === "out" && entry.in_reply_to) replies.set(entry.in_reply_to, entry.message_id);
  const work = new Map<string, ConciergeWorkEntry>();
  const reported = entries.some(entry => entry.handling_state !== undefined);
  let open: string | null = null;
  for (const entry of entries) {
    if (authorKind(entry) !== "friend") continue;
    const reply = replies.get(entry.message_id);
    const reason = entry.handling_reason ?? entry.reason;
    const settled = (state: ConciergeWork) => work.set(entry.message_id, { work: state, ...(reply ? { reply } : {}), ...(reason ? { reason } : {}) });
    if (entry.handling_state) settled(entry.handling_state);
    else if (entry.outcome === "refused") settled("refused");
    else if (entry.outcome === "answered" || reply) settled("answered");
    else open = entry.message_id;
  }
  if (!reported && open && (concierge.working || concierge.missing)) work.set(open, { work: concierge.working ? "working" : "waiting" });
  return work;
}

const ATTACHMENT_LINE =/^\[Neighbor attachment: .* \(\d+ bytes\), handle ([0-9a-f]+)\]$/;

/** The words a person reads: the agent's attachment lines go when their files show as chips, and stay as the only trace otherwise. */
export function readableBody(body: string, fileIds: ReadonlySet<string>): string {
  if (fileIds.size === 0) return body;
  return body.split("\n").filter(line => !fileIds.has(ATTACHMENT_LINE.exec(line)?.[1] ?? "")).join("\n").trimEnd();
}

/** A message continues its author's group when the same author spoke last on the same day. */
export function continuesGroup(entry: HistoryEntry, previous: HistoryEntry | null): boolean {
  if (!previous || authorKind(entry) !== authorKind(previous)) return false;
  if (new Date(entry.at).toDateString() !== new Date(previous.at).toDateString()) return false;
  const name = (value: HistoryEntry) => typeof value.author === "object" ? value.author.agent : value.author;
  return name(entry) === name(previous);
}
