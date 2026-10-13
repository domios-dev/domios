import { DOMIOS_CLOSE_TAG, parseDomiosMessage } from "./domios";
import type { ChatSend } from "./sendOrder";
import type { NormalizedChatMessage } from "./types";

export type MessagePart = { kind: "frame" | "text"; content: string; correlationId?: string };
export interface ExactSendText {
  content: string;
  correlationId?: string;
}

export function parseMessageParts(content: string): MessagePart[] {
  const parts: MessagePart[] = [];
  let offset = 0;
  while (offset < content.length) {
    const separator = parts.length > 0 ? /^\s*(?=<domios\s)/.exec(content.slice(offset))?.[0].length ?? 0 : 0;
    const start = offset + separator;
    if (!content.startsWith("<domios", start)) break;
    const close = content.indexOf(DOMIOS_CLOSE_TAG, start);
    if (close < 0) break;
    const end = close + DOMIOS_CLOSE_TAG.length;
    const frame = content.slice(start, end);
    if (!parseDomiosMessage(frame)) break;
    parts.push({ kind: "frame", content: frame });
    offset = end;
  }
  if (offset < content.length || parts.length === 0) {
    parts.push({ kind: "text", content: content.slice(offset) });
  }
  return parts;
}

export function splitExactSendTexts(content: string, payloads: readonly ExactSendText[]): MessagePart[] {
  const segments: MessagePart[] = [];
  let offset = 0;
  for (const payload of payloads) {
    if (!payload.content) continue;
    const start = content.indexOf(payload.content, offset);
    if (start < 0) continue;
    if (start > offset) segments.push(...(offset === 0 ? parseMessageParts(content.slice(offset, start)) : [{ kind: "text" as const, content: content.slice(offset, start) }]));
    segments.push(...parseMessageParts(payload.content).map((part) => ({ ...part, correlationId: payload.correlationId })));
    offset = start + payload.content.length;
  }
  if (offset === 0) return parseMessageParts(content);
  if (offset < content.length) segments.push({ kind: "text", content: content.slice(offset) });
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
    const ownPayloads = anchor < 0 ? [] : payloads.slice(anchor);
    const parts = splitExactSendTexts(row.content, ownPayloads);
    const projected = parts.length > 1 ? { ...row, messageParts: parts } : row;
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
