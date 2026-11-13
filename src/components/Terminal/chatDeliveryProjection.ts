import { normalizeChatText, timestampMs, type SendEcho } from "@codeterm/chat-core";
import { deliveryState, sendForRow, type DeliverySend } from "./chatDeliveryModel";
import type { EnrichedChatEntry } from "./chatEnrichment";

export interface DeliveryProjectionMemory {
  key: string;
  seen: Set<string>;
  queued: Set<string>;
  pending: Map<string, EnrichedChatEntry>;
}

export function queuedDeliveryIds(sends: readonly DeliverySend[], queueNew: boolean, memory: DeliveryProjectionMemory): ReadonlySet<string> {
  const current = new Set(sends.map(send => send.id));
  for (const id of memory.queued) if (!current.has(id)) memory.queued.delete(id);
  for (const send of sends) {
    if ((!memory.seen.has(send.id) && queueNew && deliveryState(send) !== "delivered")
      || send.status?.pending_reason === "held_active_turn") memory.queued.add(send.id);
  }
  memory.seen = current;
  return memory.queued;
}

export function stableDeliveryEntries(
  entries: readonly EnrichedChatEntry[], sends: readonly DeliverySend[], echoes: ReadonlyMap<string, SendEcho>, memory: DeliveryProjectionMemory,
): EnrichedChatEntry[] {
  const reconciled = sends.map(send => ({ ...send, echo: send.echo ?? echoes.get(send.id) }));
  const present = new Set<string>();
  const rows = entries.map(entry => {
    const send = sendForRow(reconciled, entry.msg) ?? reconciled.find(candidate => {
      if (present.has(candidate.id) || !candidate.echo || entry.msg.type !== "user" || entry.msg.inheritedFromSessionId != null) return false;
      const source = candidate.matchedRow?.source.source_session_id ?? candidate.echoBoundary?.sourceSessionId;
      return source != null && entry.msg.sourceSessionId === source
        && candidate.echo.timestampMs === timestampMs(entry.msg.timestamp)
        && candidate.echo.content === normalizeChatText(entry.msg.content).toLowerCase();
    });
    if (!send) return entry;
    present.add(send.id);
    const msg = entry.msg.id === send.id ? entry.msg : { ...entry.msg, id: send.id, sourceMessageId: entry.msg.sourceMessageId ?? entry.msg.id };
    const stable = msg === entry.msg ? entry : { ...entry, msg, item: { kind: "message" as const, message: msg } };
    if (msg.sourceSessionId == null) memory.pending.set(send.id, stable);
    else memory.pending.delete(send.id);
    return stable;
  });
  const byId = new Map(sends.map(send => [send.id, send]));
  for (const [id, entry] of memory.pending) {
    const send = byId.get(id);
    if (!send || send.dismissed) { memory.pending.delete(id); continue; }
    if (present.has(id)) continue;
    const index = send.held ? -1 : rows.findIndex(({ msg }) => {
      const at = timestampMs(msg.timestamp);
      return at != null && at > send.sentAtMs;
    });
    rows.splice(index < 0 ? rows.length : index, 0, entry);
  }
  return rows;
}
