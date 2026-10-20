import type { ClipMessage } from "@codeterm/chat-core";
import { messageParts, type MessagePart } from "@codeterm/chat-ui";

export interface MessagePartIndex {
  readonly clips: readonly ClipMessage[];
  partsOf(clip: ClipMessage): readonly MessagePart[];
  partsFor(messageId: string): readonly MessagePart[];
  /** A whole message, or one part of it with the part's source. */
  clipFor(id: string): ClipMessage | undefined;
  messageOf(id: string): string | undefined;
  readonly order: readonly string[];
}

/** Messages are split into parts on first use, so mounting a long transcript parses only the rows it shows. */
export function indexMessageParts(clips: readonly ClipMessage[], streamingMessageId?: string | null): MessagePartIndex {
  const clipById = new Map(clips.map((clip) => [clip.id, clip]));
  const partsById = new Map<string, readonly MessagePart[]>();
  let order: readonly string[] | null = null;
  let partOwners: Map<string, { clip: ClipMessage; part: MessagePart }> | null = null;
  const partsOf = (clip: ClipMessage) => {
    let parts = partsById.get(clip.id);
    if (!parts) {
      parts = clip.role === "assistant" && clip.id !== streamingMessageId
        ? messageParts(clip.id, clip.source)
        : [{ id: clip.id, start: 0, end: clip.source.length, renderedStart: 0, renderedEnd: clip.source.length }];
      partsById.set(clip.id, parts);
    }
    return parts;
  };
  const owners = () => partOwners ??= new Map(clips.flatMap((clip) => partsOf(clip).map((part) => [part.id, { clip, part }] as const)));
  return {
    clips,
    partsOf,
    partsFor: (messageId) => {
      const clip = clipById.get(messageId);
      return clip ? partsOf(clip) : [];
    },
    clipFor: (id) => {
      const clip = clipById.get(id);
      if (clip) return clip;
      const owner = owners().get(id);
      return owner && { ...owner.clip, source: owner.clip.source.slice(owner.part.start, owner.part.end) };
    },
    messageOf: (id) => clipById.has(id) ? id : owners().get(id)?.clip.id,
    get order() {
      return order ??= clips.flatMap((clip) => partsOf(clip).map((part) => part.id));
    },
  };
}

export function clipsForParts(index: MessagePartIndex, ids: readonly string[]): ClipMessage[] {
  const wanted = new Set(ids);
  return index.clips.flatMap((clip) => {
    const parts = index.partsOf(clip);
    if (wanted.has(clip.id) || parts.every((part) => wanted.has(part.id))) return [clip];
    return parts.filter((part) => wanted.has(part.id)).map((part) => index.clipFor(part.id)!);
  });
}

export function partSelectionSummary(index: MessagePartIndex, ids: readonly string[]) {
  const selected = new Set(ids);
  let partial = false;
  let messages = 0;
  if (ids.length === 0) return { selected, partial, count: 0 };
  for (const clip of index.clips) {
    const parts = index.partsOf(clip);
    if (parts.every((part) => selected.has(part.id))) { selected.add(clip.id); messages++; }
    else if (parts.some((part) => selected.has(part.id))) partial = true;
  }
  return { selected, partial, count: partial ? ids.length : messages };
}
