import type { ChatSend } from "./sendOrder";
import type { NormalizedChatMessage } from "./types";
import type { MessageSegment } from "./envelope";

export interface ExactSendText {
  content: string;
  correlationId?: string;
}

export function splitExactSendTexts(content: string, payloads: readonly ExactSendText[]): MessageSegment[] {
  const segments: MessageSegment[] = [];
  let offset = 0;
  for (const payload of payloads) {
    if (!payload.content) continue;
    const start = content.indexOf(payload.content, offset);
    if (start < 0) continue;
    if (start > offset) segments.push({ type: "text", text: content.slice(offset, start) });
    segments.push({ type: "text", text: payload.content, correlationId: payload.correlationId });
    offset = start + payload.content.length;
  }
  if (offset === 0) return [{ type: "text", text: content }];
  if (offset < content.length) segments.push({ type: "text", text: content.slice(offset) });
  return segments;
}

const cachedParts = new WeakMap<NormalizedChatMessage, { key: string; row: NormalizedChatMessage }>();

export function projectMessageParts(
  rows: readonly NormalizedChatMessage[], payloads: readonly ExactSendText[], key: string,
): NormalizedChatMessage[] {
  return rows.map((row) => {
    if (row.type !== "user") return row;
    const cached = cachedParts.get(row);
    if (cached?.key === key) return cached.row;
    const anchor = row.correlationId == null ? -1 : payloads.findIndex((payload) => payload.correlationId === row.correlationId);
    if (anchor < 0) return row;
    const ownPayloads = payloads.slice(anchor);
    const segments = (row.segments ?? [{ type: "text", text: row.content }]).flatMap((segment): MessageSegment[] => {
      if (segment.type !== "text" && segment.type !== "paste") return [segment];
      return splitExactSendTexts(segment.type === "text" ? segment.text : segment.body, ownPayloads);
    });
    const projected = { ...row, segments };
    cachedParts.set(row, { key, row: projected });
    return projected;
  });
}

const ledgerTexts = new WeakMap<readonly ChatSend[], { texts: ExactSendText[]; key: string }>();

export function projectTranscriptParts(
  rows: readonly NormalizedChatMessage[], sends: readonly ChatSend[],
): NormalizedChatMessage[] {
  let ledger = ledgerTexts.get(sends);
  if (!ledger) {
    const texts = sends.filter((send) => "flowAtMs" in send && typeof send.flowAtMs === "number");
    ledger = { texts, key: JSON.stringify(texts.map((send) => [send.correlationId, send.content])) };
    ledgerTexts.set(sends, ledger);
  }
  return projectMessageParts(rows, ledger.texts, ledger.key);
}
