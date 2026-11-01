import { envelopeRows } from "@codeterm/chat-core";
import type { FrameKind, MessageSegment, NormalizedChatMessage as ChatMessage, TimelineItem } from "@codeterm/chat-core";
import { detectChatPrefix, framePrefix, type ChatOrigin } from "../../lib/chatPrefixes";

const TIP = /\s*<tip>([\s\S]*?)<\/tip>\s*$/;

/** The daemon parses only the frame kinds it knows, so a Neighbors frame reaches the chat as an unknown frame. It is a
 *  routed frame like any other: lift it into one, with its host tip split off the body. */
function liftNeighborFrames(segments: readonly MessageSegment[]): MessageSegment[] {
  return segments.map((segment): MessageSegment => {
    if (segment.type !== "unknown_frame" || segment.attrs.from !== "neighbor") return segment;
    const tip = TIP.exec(segment.body);
    return { type: "frame", kind: "neighbor" as FrameKind, from: "neighbor", attrs: segment.attrs,
      body: tip ? segment.body.slice(0, tip.index) : segment.body, tip: tip ? tip[1].trim() : null };
  });
}

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
    const segments = liftNeighborFrames(raw.segments) as typeof raw.segments;
    const hasFrames = segments.some((segment) => segment.type === "frame");
    // With no closed frame and one correlation there is nothing to split: the message keeps its exact text, partial frames included.
    const single = !hasFrames && new Set(segments.map((segment) => segment.correlationId)).size <= 1;
    const rows = single ? [{ content: raw.content, segments }] : envelopeRows(segments);
    if (!rows.length && raw.attachments?.length) rows.push({ content: raw.content, segments });
    return rows.map((row, index) => {
      const prefix = row.frame ? framePrefix({ ...row.frame.attrs, from: row.frame.from }, row.content, row.frame.tip) : detectChatPrefix(row.content);
      // A Neighbors frame's header and host instruction fold into its origin header; the bubble shows only the friend's words.
      // A daemon-parsed frame may carry its own text segments; they still hold the escaped words, so they give way too.
      const neighborBody = row.frame?.from === "neighbor" && prefix && !(row.frame.segments ?? []).some((segment) => segment.type === "attachment");
      const msg: ChatMessage = {
        ...raw, id: `${raw.id}-segment-${index}`, content: prefix?.body ?? row.content,
        sourceMessageId: raw.sourceMessageId ?? raw.id,
        segments: neighborBody ? [{ type: "text", text: prefix.body }] as typeof row.segments : row.segments,
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
