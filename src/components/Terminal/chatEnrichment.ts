import { envelopeRows, parseDomiosMessage } from "@codeterm/chat-core";
import type { FrameKind, MessageSegment, NormalizedChatMessage as ChatMessage, TimelineItem } from "@codeterm/chat-core";
import { detectChatPrefix, framePrefix, type ChatOrigin } from "../../lib/chatPrefixes";

const TIP = /\s*<tip>([\s\S]*?)<\/tip>\s*$/;

/** The daemon parses only the frame kinds it knows, so Neighbors and CLI frames reach the chat as unknown frames.
 *  Lift them into routed frames like any other, with any host tip split off the body. */
function liftHostFrames(segments: readonly MessageSegment[]): MessageSegment[] {
  return segments.map((segment): MessageSegment => {
    if (segment.type !== "unknown_frame" || (segment.attrs.from !== "neighbor" && segment.attrs.from !== "cli")) return segment;
    const tip = TIP.exec(segment.body);
    const from = segment.attrs.from;
    return { type: "frame", kind: (from === "cli" ? "system" : "neighbor") as FrameKind, from, attrs: segment.attrs,
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

const AGENT_SETUP = /^\s*(<session_context(?:\s[^>]*)?>[\s\S]*?<\/session_context>)\s*/;

/** An agent CLI's own setup block, replayed as a user message on load, reads as quiet context; prose quoting the tag stays a bubble. */
function splitAgentSetup(raw: ChatMessage): { notice: EnrichedChatEntry; rest: ChatMessage | null } | null {
  const match = AGENT_SETUP.exec(raw.content);
  if (!match) return null;
  const words = raw.content.slice(match[0].length);
  const message: ChatMessage = { id: words ? `${raw.id}-agent-setup` : raw.id, type: "system", systemSubtype: "environment_context",
    content: match[1], collapsed: true, timestamp: raw.timestamp };
  const notice = { msg: message, item: { kind: "message" as const, message }, origin: undefined };
  if (!words) return { notice, rest: null };
  const segments = raw.segments?.flatMap((segment) => {
    if (segment.type !== "text" || !segment.text.startsWith(match[0])) return [segment];
    const text = segment.text.slice(match[0].length);
    return text ? [{ ...segment, text }] : [];
  });
  return { notice, rest: { ...raw, content: words, segments } };
}

function enrichUserMessage(raw: ChatMessage, rawItem: TimelineItem): EnrichedChatEntry[] {
  if (raw.inboundKind === "system" && raw.fromContext === "provider-handoff") {
    return [{ msg: raw, item: rawItem, origin: undefined }];
  }
  const setup = splitAgentSetup(raw);
  if (setup) {
    if (!setup.rest) return [setup.notice];
    return [setup.notice, ...enrichUserMessage(setup.rest, { kind: "message", message: setup.rest })];
  }
  if (raw.segments) {
    const segments = liftHostFrames(raw.segments) as typeof raw.segments;
    const hasFrames = segments.some((segment) => segment.type === "frame");
    // With no closed frame and one correlation there is nothing to split: the message keeps its exact text, partial frames included.
    const single = !hasFrames && new Set(segments.map((segment) => segment.correlationId)).size <= 1;
    const rows = single ? [{ content: raw.content, segments }] : envelopeRows(segments);
    if (!rows.length && raw.attachments?.length) rows.push({ content: raw.content, segments });
    return rows.map((row, index) => {
      const curator = row.segments.length === 1 && row.segments[0].type === "unknown_frame"
        && row.segments[0].attrs.from === "curator" && raw.content.startsWith(row.segments[0].raw)
        ? row.segments[0] : undefined;
      const curatorWire = curator ? parseDomiosMessage(curator.raw) : null;
      const curatorTip = curatorWire ? TIP.exec(curatorWire.content) : null;
      const frame = row.frame ?? (curator && curatorWire ? { from: "curator", attrs: curator.attrs,
        body: curatorWire.body, tip: curatorTip?.[1] ?? null } : undefined);
      const prefix = frame ? framePrefix({ ...frame.attrs, from: frame.from }, row.frame ? row.content : frame.body, frame.tip) : detectChatPrefix(row.content);
      // A Neighbors frame's header and host instruction fold into its origin header; the bubble shows only the friend's words.
      // A daemon-parsed frame may carry its own text segments; they still hold the escaped words, so they give way too.
      const neighborBody = row.frame?.from === "neighbor" && prefix && !(row.frame.segments ?? []).some((segment) => segment.type === "attachment");
      const msg: ChatMessage = {
        ...raw, id: `${raw.id}-segment-${index}`, content: prefix?.body ?? row.content,
        sourceMessageId: raw.sourceMessageId ?? raw.id,
        segments: (neighborBody || curator) && prefix ? [{ type: "text", text: prefix.body }] as typeof row.segments : row.segments,
        routing: frame ? { kind: frame.from, label: frame.attrs.label, model: frame.attrs.model, tab: frame.attrs.tab, tip: frame.tip } : undefined,
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
  if ((m.inboundKind as string | undefined) === "cli") {
    return [{ msg: m, item, origin: { kind: "system", context: "CLI" } }];
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

/** A system notice (a `from="system"` or `from="cli"` frame) with no words and no files has nothing to show;
 *  rendering it would leave a bare "System · CLI" card. */
function emptySystemNotice(entry: EnrichedChatEntry): boolean {
  if (entry.origin?.kind !== "system" && entry.msg.inboundKind !== "system") return false;
  const files = (entry.msg.segments ?? []).some((segment) => segment.type === "attachment" || (segment.type === "paste" && Boolean(segment.body.trim())));
  return !entry.msg.content.trim() && !files && !entry.msg.attachments?.length;
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
    if (m.type === "system" && m.systemSubtype === "provider_handoff") continue;
    if (m.type !== "user") {
      append({ msg: m, item, origin: undefined });
      continue;
    }
    const cached = cache.get(m);
    if (cached) {
      for (const entry of cached) append(entry);
      continue;
    }
    const entries = enrichUserMessage(m, item).filter((entry) => !emptySystemNotice(entry));
    cache.set(m, entries);
    for (const entry of entries) append(entry);
  }
  return out;
}
