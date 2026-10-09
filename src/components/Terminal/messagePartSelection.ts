import type { ClipMessage } from "@codeterm/chat-core";
import { messageParts, type MessagePart } from "@codeterm/chat-ui";

export function indexMessageParts(clips: readonly ClipMessage[], streamingMessageId?: string | null) {
  const rows = clips.map((clip) => ({
    clip,
    parts: clip.role === "assistant" && clip.id !== streamingMessageId ? messageParts(clip.id, clip.source) : [{ id: clip.id, start: 0, end: clip.source.length, renderedStart: 0, renderedEnd: clip.source.length }],
  }));
  const byMessage = new Map<string, readonly MessagePart[]>(rows.map(({ clip, parts }) => [clip.id, parts]));
  const byId = new Map<string, ClipMessage>();
  for (const { clip, parts } of rows) {
    byId.set(clip.id, clip);
    for (const part of parts) byId.set(part.id, { ...clip, source: clip.source.slice(part.start, part.end) });
  }
  return { rows, byMessage, byId, order: rows.flatMap(({ parts }) => parts.map((part) => part.id)) };
}

export type MessagePartIndex = ReturnType<typeof indexMessageParts>;

export function clipsForParts(index: MessagePartIndex, ids: readonly string[]): ClipMessage[] {
  const wanted = new Set(ids);
  return index.rows.flatMap(({ clip, parts }) => {
    if (wanted.has(clip.id) || parts.every((part) => wanted.has(part.id))) return [clip];
    return parts.filter((part) => wanted.has(part.id)).map((part) => index.byId.get(part.id)!);
  });
}

export function partSelectionSummary(index: MessagePartIndex, ids: readonly string[]) {
  const selected = new Set(ids);
  let partial = false;
  let messages = 0;
  for (const { clip, parts } of index.rows) {
    if (parts.every((part) => selected.has(part.id))) { selected.add(clip.id); messages++; }
    else if (parts.some((part) => selected.has(part.id))) partial = true;
  }
  return { selected, partial, count: partial ? ids.length : messages };
}
