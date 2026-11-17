import { normalizeChatText, pendingSendPosition, timestampMs, type SendEcho } from "@codeterm/chat-core";
import { ACTIVE_TURN_HOLD, deliveryState, sendForRow, sendsForRow, type DeliverySend, type DeliveryState } from "./chatDeliveryModel";
import type { EnrichedChatEntry } from "./chatEnrichment";

export interface DeliveryProjectionMemory {
  key: string;
  seen: Set<string>;
  queued: Set<string>;
  /** Turn ends observed while each queued send waited. */
  turnEnds: Map<string, number>;
  turnRunning: boolean;
  /** Sends whose wait behind a turn ended; they keep their send-time slot. */
  released: Set<string>;
  pending: Map<string, EnrichedChatEntry>;
}

export function createDeliveryProjectionMemory(key: string): DeliveryProjectionMemory {
  return { key, seen: new Set(), queued: new Set(), turnEnds: new Map(), turnRunning: false, released: new Set(), pending: new Map() };
}

// The turn it queued behind ends first; queued input is submitted then, so only a
// following turn that also ends without its echo proves its row is not coming here.
const TURN_ENDS_BEFORE_UNECHOED_RELEASE = 2;

// A delivered receipt alone keeps the tail: a loaded claimed record places it exactly, and
// releasing before the echo would move it twice when the record cannot be located.
function tailWaitEnded(send: DeliverySend, state: DeliveryState, turnEnds: number): boolean {
  return Boolean(send.dismissed) || send.echo != null || state === "failed"
    || ((state === "uncertain" || state === "delivered") && turnEnds >= TURN_ENDS_BEFORE_UNECHOED_RELEASE);
}

/** Sends drawn after the running turn's output. Only input still in flight joins, so a
 *  reload never pins settled sends to the tail; one that fails, or stays unconfirmed after
 *  the turn that should have taken it, moves once to its send time and stays there. */
export function queuedDeliveryIds(sends: readonly DeliverySend[], turnRunning: boolean, memory: DeliveryProjectionMemory): ReadonlySet<string> {
  const current = new Set(sends.map(send => send.id));
  const turnEnded = memory.turnRunning && !turnRunning;
  memory.turnRunning = turnRunning;
  for (const id of memory.queued) if (!current.has(id)) memory.queued.delete(id);
  for (const id of memory.turnEnds.keys()) if (!memory.queued.has(id)) memory.turnEnds.delete(id);
  for (const id of memory.released) if (!current.has(id)) memory.released.delete(id);
  for (const send of sends) {
    if (memory.released.has(send.id)) continue;
    const state = deliveryState(send);
    const inFlight = state === "sending" || state === "queued";
    if (send.status?.pending_reason === ACTIVE_TURN_HOLD
      || (!memory.seen.has(send.id) && inFlight && (turnRunning || send.wasHeld))) memory.queued.add(send.id);
    if (!memory.queued.has(send.id)) continue;
    const turnEnds = (memory.turnEnds.get(send.id) ?? 0) + (turnEnded ? 1 : 0);
    if (tailWaitEnded(send, state, turnEnds)) {
      memory.queued.delete(send.id);
      memory.turnEnds.delete(send.id);
      memory.released.add(send.id);
    } else memory.turnEnds.set(send.id, turnEnds);
  }
  memory.seen = current;
  return memory.queued;
}

export function stableDeliveryEntries(
  entries: readonly EnrichedChatEntry[], sends: readonly DeliverySend[], echoes: ReadonlyMap<string, SendEcho>, memory: DeliveryProjectionMemory,
  outside: ReadonlySet<string> = new Set(),
): EnrichedChatEntry[] {
  const reconciled = sends.map(send => ({ ...send, echo: send.echo ?? echoes.get(send.id) }));
  const present = new Set<string>();
  const rows = entries.map(entry => {
    const send = sendForRow(reconciled, entry.msg) ?? reconciled.find(candidate => {
      if (present.has(candidate.id) || candidate.matchedRow || !candidate.echo || entry.msg.type !== "user" || entry.msg.inheritedFromSessionId != null) return false;
      const source = candidate.echoBoundary?.sourceSessionId;
      return source != null && entry.msg.sourceSessionId === source
        && candidate.echo.timestampMs === timestampMs(entry.msg.timestamp)
        && candidate.echo.content === normalizeChatText(entry.msg.content).toLowerCase();
    });
    // Several rows of one record keep their own keys; only the first carries the send's identity.
    if (!send || present.has(send.id)) return entry;
    present.add(send.id);
    const names = Object.values(send.draft?.capsules ?? {}).filter(capsule => capsule.path && capsule.displayName)
      .map(capsule => [capsule.path, capsule.displayName!] as const);
    const msg = entry.msg.id === send.id && !names.length ? entry.msg : { ...entry.msg, id: send.id,
      sourceMessageId: entry.msg.sourceMessageId ?? entry.msg.id, ...(names.length ? { attachmentNames: Object.fromEntries(names) } : {}) };
    const stable = msg === entry.msg ? entry : { ...entry, msg, item: { kind: "message" as const, message: msg } };
    if (entry.msg.id === send.id && entry.msg.sourceMessageId == null) memory.pending.set(send.id, stable);
    else memory.pending.delete(send.id);
    return stable;
  });
  const byId = new Map(reconciled.map(send => [send.id, send]));
  for (const [id, entry] of memory.pending) {
    const send = byId.get(id);
    if (!send || send.dismissed || outside.has(id)) { memory.pending.delete(id); continue; }
    if (present.has(id)) continue;
    if (echoes.has(id) && entries.some(entry => sendsForRow([send], entry.msg).length > 0)) {
      memory.pending.delete(id);
      continue;
    }
    const index = pendingSendPosition(rows.map(row => row.msg), send);
    rows.splice(index, 0, entry);
  }
  return rows;
}
