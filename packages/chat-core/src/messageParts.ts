import { markVoice, parseMarker } from "./markers";
import { attachmentMarkerSegments, formatAttachmentMarker, type AttachmentMarker } from "./attachmentMarker";
import { DOMIOS_CLOSE_TAG, parseDomiosMessage } from "./domios";
import type { ChatSend } from "./sendOrder";
import type { NormalizedChatMessage } from "./types";

export type MessagePart = ({ kind: "frame" | "text" } | { kind: "attachment"; attachments: AttachmentMarker[] }) & { content: string; correlationId?: string };
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
  const remainder = content.slice(offset);
  if (remainder.trim() || parts.length === 0) {
    parts.push(...parseAttachmentParts(remainder));
  }
  return parts;
}

function parseAttachmentParts(content: string): MessagePart[] {
  const voice = parseMarker(content);
  if (voice?.kind === "voice") {
    return [{ kind: "attachment", attachments: [{ kind: "voice", path: voice.attrs.file, label: "voice" }],
      content: markVoice(voice.attrs.file, voice.attrs.status === "untranscribed" ? null : voice.payload,
        voice.attrs.status === "untranscribed" ? voice.payload : null) }];
  }
  if (!content.includes("<domios")) return [{ kind: "text", content }];
  const parts: MessagePart[] = [];
  for (const segment of attachmentMarkerSegments(content)) {
    const previous = parts[parts.length - 1];
    if ("text" in segment) {
      if (previous?.kind === "attachment" && !segment.text.trim()) previous.content += segment.text;
      else parts.push({ kind: "text", content: segment.text });
    } else if (segment.attachment.reply === undefined) {
      const marker = formatAttachmentMarker(segment.attachment.path, segment.attachment.kind, segment.attachment.label);
      if (previous?.kind === "attachment") {
        previous.attachments.push(segment.attachment);
        previous.content += marker;
      } else parts.push({ kind: "attachment", attachments: [segment.attachment], content: marker });
    } else {
      const marker = formatAttachmentMarker(segment.attachment.path, segment.attachment.kind, segment.attachment.label,
        { ref: segment.attachment.reply, excerpt: segment.attachment.excerpt ?? "" });
      if (previous?.kind === "text") previous.content += marker;
      else parts.push({ kind: "text", content: marker });
    }
  }
  return parts.length ? parts : [{ kind: "text", content }];
}

// Claude Code wraps a pasted send in this envelope; split around an exact send, the envelope alone is not a row.
const PASTE_ENVELOPE_TAG = /<[\\/]{0,2}pasted_content(?:\s+id="[^"]*")?\s*>/g;

export function messageRowParts(parts: readonly MessagePart[]): MessagePart[] {
  const rows: MessagePart[] = [];
  let attachmentRow = false;
  for (const part of parts) {
    if (part.kind === "text" && !part.content.replace(PASTE_ENVELOPE_TAG, "").trim()) continue;
    const previous = rows[rows.length - 1];
    if (part.kind !== "frame" && previous?.kind === "text" && previous.correlationId === part.correlationId && (attachmentRow || part.kind === "attachment")) {
      previous.content += part.content;
      attachmentRow = true;
    } else {
      rows.push(part.kind === "attachment" ? { kind: "text", content: part.content, correlationId: part.correlationId } : { ...part });
      attachmentRow = part.kind === "attachment";
    }
  }
  return rows;
}

export function splitExactSendTexts(content: string, payloads: readonly ExactSendText[]): MessagePart[] {
  const segments: MessagePart[] = [];
  let offset = 0;
  for (const payload of payloads) {
    if (!payload.content) continue;
    const start = content.indexOf(payload.content, offset);
    if (start < 0) continue;
    if (start > offset) segments.push(...parseMessageParts(content.slice(offset, start)));
    segments.push(...parseMessageParts(payload.content).map((part) => ({ ...part, correlationId: payload.correlationId })));
    offset = start + payload.content.length;
  }
  if (offset === 0) return parseMessageParts(content);
  if (offset < content.length) segments.push(...parseMessageParts(content.slice(offset)));
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
    const projected = parts.length > 1 || parts.some((part) => part.kind === "frame") ? { ...row, messageParts: parts } : row;
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
