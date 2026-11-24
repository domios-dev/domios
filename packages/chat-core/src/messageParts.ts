import { matchesMatchedRow, type ChatSend } from "./sendOrder";
import type { NormalizedChatMessage } from "./types";
import type { MessageSegment } from "./envelope";
import { userEchoBody } from "./userEchoBody";

export interface ExactSendText {
  content: string;
  correlationId?: string;
  segments?: MessageSegment[];
}

function sendSegments(payload: ExactSendText): MessageSegment[] {
  return payload.segments?.length
    ? payload.segments.map((segment) => ({ ...segment, correlationId: payload.correlationId }))
    : [{ type: "text", text: payload.content, correlationId: payload.correlationId }];
}

export function splitExactSendTexts(content: string, payloads: readonly ExactSendText[]): MessageSegment[] {
  const exact = payloads.find((payload) => payload.content === content);
  if (exact) return sendSegments(exact);
  const segments: MessageSegment[] = [];
  let offset = 0;
  let matched = 0;
  for (const payload of payloads) {
    if (!payload.content) continue;
    const whitespace = /^\s*/.exec(content.slice(offset))?.[0].length ?? 0;
    const start = offset + whitespace;
    if (!content.startsWith(payload.content, start)) continue;
    if (start > offset) segments.push({ type: "text", text: content.slice(offset, start) });
    segments.push(...sendSegments(payload));
    matched += 1;
    offset = start + payload.content.length;
  }
  if (matched < 2 || content.slice(offset).trim()) return [{ type: "text", text: content }];
  if (offset < content.length) segments.push({ type: "text", text: content.slice(offset) });
  return segments;
}

const cachedParts = new WeakMap<NormalizedChatMessage, { key: string; payloads: readonly ExactSendText[]; row: NormalizedChatMessage }>();

export function projectMessageParts(
  rows: readonly NormalizedChatMessage[], payloads: readonly ExactSendText[], key: string,
): NormalizedChatMessage[] {
  return rows.map((row) => {
    if (row.type !== "user") return row;
    const cached = cachedParts.get(row);
    if (cached?.key === key && cached.payloads.length === payloads.length
      && cached.payloads.every((payload, index) => payload.segments === payloads[index].segments)) return cached.row;
    const anchor = row.correlationId == null ? -1 : payloads.findIndex((payload) => payload.correlationId === row.correlationId);
    if (anchor < 0) return row;
    const ownPayloads = payloads.slice(anchor);
    const segments = (row.segments ?? [{ type: "text", text: row.content }]).flatMap((segment): MessageSegment[] => {
      if (segment.type !== "text" && segment.type !== "paste") return [segment];
      return splitExactSendTexts(segment.type === "text" ? segment.text : segment.body, ownPayloads);
    });
    const projected = { ...row, segments };
    cachedParts.set(row, { key, payloads, row: projected });
    return projected;
  });
}

const ledgerTexts = new WeakMap<readonly ChatSend[], {
  texts: ChatSend[];
  key: string;
  routedSends: Map<string, ChatSend>;
  routedClaims: Map<string, ChatSend>;
}>();

function structuredRouteKey(session: string, id: string): string {
  return JSON.stringify([session, id]);
}

const cachedRoutedParts = new WeakMap<NormalizedChatMessage, { send: ChatSend; row: NormalizedChatMessage }>();

export function projectTranscriptParts(
  rows: readonly NormalizedChatMessage[], sends: readonly ChatSend[],
  sourceSessionId?: string | null,
): NormalizedChatMessage[] {
  let ledger = ledgerTexts.get(sends);
  if (!ledger) {
    const texts = sends.filter((send) => "flowAtMs" in send && typeof send.flowAtMs === "number");
    const routedSends = new Map<string, ChatSend>();
    const routedClaims = new Map<string, ChatSend>();
    for (const send of texts) {
      if (!send.segments?.some(segment => segment.type === "frame")) continue;
      if (!send.matchedRow) routedSends.set(send.id, send);
      const source = send.matchedRow?.source;
      if (source?.record.kind === "structured") {
        routedClaims.set(structuredRouteKey(source.source_session_id, source.record.item_id), send);
      }
    }
    ledger = { texts, routedSends, routedClaims, key: JSON.stringify(texts.map((send) => [send.correlationId, send.content])) };
    ledgerTexts.set(sends, ledger);
  }
  const projected = projectMessageParts(rows, ledger.texts, ledger.key);
  const { routedSends, routedClaims } = ledger;
  if (routedSends.size === 0 && routedClaims.size === 0) return projected;
  return projected.map(row => {
    if (row.type !== "user" || row.correlationId != null || row.sourceSessionId == null || row.inheritedFromSessionId != null) return row;
    const session = row.sourceSessionId;
    const send = routedClaims.get(structuredRouteKey(session, row.id))
      ?? (row.runtimeSourceItemId == null ? undefined : routedClaims.get(structuredRouteKey(session, row.runtimeSourceItemId)))
      ?? (session === sourceSessionId ? routedSends.get(row.id) : undefined);
    if (!send || (send.matchedRow && !matchesMatchedRow(row, send.matchedRow))) return row;
    const cached = cachedRoutedParts.get(row);
    if (cached?.send === send) return cached.row;
    const hostFrame = row.inboundKind != null && row.segments?.length === 1 && row.segments[0].type === "frame";
    const plainEcho = row.inboundKind == null
      && !row.segments?.some(segment => segment.type !== "text" && segment.type !== "paste")
      && userEchoBody(row.content) === userEchoBody(send.content);
    if (!hostFrame && !plainEcho) return row;
    const routed = { ...row, correlationId: send.correlationId, segments: hostFrame ? row.segments : sendSegments(send) };
    cachedRoutedParts.set(row, { send, row: routed });
    return routed;
  });
}
