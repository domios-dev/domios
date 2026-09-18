import type { NormalizedChatMessage as ChatMessage, TimelineItem } from "@codeterm/chat-core";
import { detectChatPrefix, splitChatPrefixedMessages, type ChatOrigin } from "../../lib/chatPrefixes";

export interface EnrichedChatEntry {
  msg: ChatMessage;
  item: TimelineItem;
  origin: ChatOrigin | undefined;
}

/** Keyed by message identity: entries are pure derivations of the message, and
 *  reusing them keeps MessageBubble memo props stable across streamed chunks. */
export type EnrichmentCache = WeakMap<ChatMessage, EnrichedChatEntry[]>;

function enrichUserMessage(m: ChatMessage, item: TimelineItem): EnrichedChatEntry[] {
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
  const splitPrefixes = splitChatPrefixedMessages(m.content);
  if (splitPrefixes && splitPrefixes.length > 1) {
    return splitPrefixes.map((prefix, index) => {
      const msg = { ...m, id: `${m.id}-prefix-${index}`, content: prefix.body };
      return { msg, item: { kind: "message" as const, message: msg }, origin: prefix.origin };
    });
  }
  const prefix = detectChatPrefix(m.content);
  if (!prefix) return [{ msg: m, item, origin: undefined }];
  const msg = { ...m, content: prefix.body };
  return [
    {
      msg,
      item: item.kind === "message" ? { kind: "message" as const, message: msg } : item,
      origin: prefix.origin,
    },
  ];
}

/** One rendered row per message identity. A message object that reaches the
 *  projection twice (an absorbed tool member re-emitted, a re-keyed row) would
 *  otherwise be pushed once per occurrence — including once per cache hit. */
export function enrichTimelineItems(visible: TimelineItem[], cache: EnrichmentCache): EnrichedChatEntry[] {
  const out: EnrichedChatEntry[] = [];
  const emitted = new Set<string>();
  const append = (entry: EnrichedChatEntry): void => {
    if (emitted.has(entry.msg.id)) return;
    emitted.add(entry.msg.id);
    out.push(entry);
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
