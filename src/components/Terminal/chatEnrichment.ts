import { envelopeRows } from "@codeterm/chat-core";
import type { NormalizedChatMessage as ChatMessage, TimelineItem } from "@codeterm/chat-core";
import { detectChatPrefix, framePrefix, type ChatOrigin } from "../../lib/chatPrefixes";

export interface EnrichedChatEntry {
  msg: ChatMessage;
  item: TimelineItem;
  origin: ChatOrigin | undefined;
}

/** Keyed by message identity: entries are pure derivations of the message, and
 *  reusing them keeps MessageBubble memo props stable across streamed chunks. */
export type EnrichmentCache = WeakMap<ChatMessage, EnrichedChatEntry[]>;

function enrichUserMessage(raw: ChatMessage, rawItem: TimelineItem): EnrichedChatEntry[] {
  if (raw.segments) {
    const hasFrames = raw.segments.some((segment) => segment.type === "frame");
    const rows = envelopeRows(raw.segments);
    if (!rows.length && raw.attachments?.length) rows.push({ content: raw.content, segments: raw.segments });
    return rows.map((row, index) => {
      const prefix = row.frame ? framePrefix({ ...row.frame.attrs, from: row.frame.from }, row.content) : detectChatPrefix(row.content);
      const msg: ChatMessage = {
        ...raw, id: `${raw.id}-segment-${index}`, content: prefix?.body ?? row.content,
        sourceMessageId: raw.sourceMessageId ?? raw.id,
        segments: row.segments,
        correlationId: row.correlationId ?? (index === 0 ? raw.correlationId : undefined),
        inboundKind: row.frame || !hasFrames ? raw.inboundKind : undefined,
        attachments: index === 0 ? raw.attachments : undefined,
        ...(hasFrames ? {
          fromLabel: undefined, fromTab: undefined, fromModel: undefined, fromMesh: undefined,
          fromPurpose: undefined, fromContext: undefined, fromPlugin: undefined,
        } : {}),
      };
      return { msg, item: { kind: "message" as const, message: msg }, origin: prefix?.origin };
    });
  }
  const prefix = detectChatPrefix(raw.content);
  const m = prefix ? { ...raw, content: prefix.body } : raw;
  const item = m === raw || rawItem.kind !== "message" ? rawItem : { kind: "message" as const, message: m };
  const pluginId = m.fromPlugin?.trim();
  if (pluginId) {
    const origin: ChatOrigin = {
      kind: "plugin",
      pluginId,
      name: m.fromLabel,
      iconSvg: m.pluginIconSvg,
    };
    return [{ msg: m, item, origin }];
  }
  if (!prefix) return [{ msg: m, item, origin: undefined }];
  return [
    {
      msg: m,
      item: item.kind === "message" ? { kind: "message" as const, message: m } : item,
      origin: prefix.origin,
    },
  ];
}

/** One rendered row per message identity. A duplicate occurrence of the same
 *  message object is dropped; a distinct object with grown content is a streamed
 *  update and replaces the stale row in place. */
export function enrichTimelineItems(visible: TimelineItem[], cache: EnrichmentCache): EnrichedChatEntry[] {
  const out: EnrichedChatEntry[] = [];
  const emittedAt = new Map<string, number>();
  const append = (entry: EnrichedChatEntry): void => {
    const at = emittedAt.get(entry.msg.id);
    if (at === undefined) {
      emittedAt.set(entry.msg.id, out.length);
      out.push(entry);
      return;
    }
    const previous = out[at];
    if (previous.msg === entry.msg || previous.msg.content === entry.msg.content) return;
    out[at] = entry;
  };
  for (const item of visible) {
    const m = item.kind === "message" ? item.message : item.event.message;
    if (m.type !== "user") {
      append({ msg: m, item, origin: undefined });
      continue;
    }
    const cached = cache.get(m);
    if (cached) {
      for (const entry of cached) append(entry);
      continue;
    }
    const entries = enrichUserMessage(m, item);
    cache.set(m, entries);
    for (const entry of entries) append(entry);
  }
  return out;
}
