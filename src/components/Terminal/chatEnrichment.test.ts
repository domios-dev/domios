import type { NormalizedChatMessage, TimelineItem } from "@codeterm/chat-core";
import { enrichTimelineItems, type EnrichmentCache } from "./chatEnrichment";

function assert(value: unknown, label: string): asserts value {
  if (!value) throw new Error(label);
}

function user(id: string, content: string): NormalizedChatMessage {
  return { id, type: "user", content };
}

function messageItem(message: NormalizedChatMessage): TimelineItem {
  return { kind: "message", message };
}

// --- plain entries pass the message and item through untouched ---
{
  const cache: EnrichmentCache = new WeakMap();
  const assistant: NormalizedChatMessage = { id: "a-1", type: "assistant", content: "hi" };
  const item = messageItem(assistant);
  const [entry] = enrichTimelineItems([item], cache);
  assert(entry.msg === assistant && entry.item === item && entry.origin === undefined, "assistant passes through");
}

// --- unchanged user messages reuse identical derived entries across calls ---
{
  const cache: EnrichmentCache = new WeakMap();
  const plain = user("u-1", "just text");
  const prefixed = user("u-2", "-=-codeterm:telegram user=42-=-hello from tg");
  const items = [messageItem(plain), messageItem(prefixed)];

  const first = enrichTimelineItems(items, cache);
  const second = enrichTimelineItems(items, cache);

  assert(second[0].msg === first[0].msg, "plain user msg identity is stable");
  assert(second[1].msg === first[1].msg, "prefixed user msg identity is stable across calls");
  assert(second[1].item === first[1].item, "prefixed user item identity is stable across calls");
  assert(second[1].origin === first[1].origin, "prefixed user origin identity is stable across calls");
  assert(first[1].msg.content === "hello from tg", "prefix body extracted");
  assert(first[1].origin != null, "prefix origin detected");
}

// --- a changed message object produces fresh derived entries ---
{
  const cache: EnrichmentCache = new WeakMap();
  const original = user("u-1", "-=-codeterm:telegram user=42-=-first");
  const [before] = enrichTimelineItems([messageItem(original)], cache);
  const updated = { ...original, content: "-=-codeterm:telegram user=42-=-second" };
  const [after] = enrichTimelineItems([messageItem(updated)], cache);
  assert(before.msg !== after.msg, "updated message derives a fresh entry");
  assert(after.msg.content === "second", "updated body extracted");
}

// --- a message that reaches the projection twice still renders one row ---
{
  const cache: EnrichmentCache = new WeakMap();
  const plain = user("u-once", "say it once");
  const item = messageItem(plain);
  const entries = enrichTimelineItems([item, item], cache);
  assert(entries.length === 1, "a repeated user message projects one bubble");
  assert(entries[0].msg.id === "u-once", "the surviving bubble keeps the message identity");
}
{
  const cache: EnrichmentCache = new WeakMap();
  const assistant: NormalizedChatMessage = { id: "a-once", type: "assistant", content: "reply" };
  const tool: TimelineItem = {
    kind: "tool",
    event: {
      id: "t-once",
      phase: "call",
      status: "success",
      kind: "generic",
      summary: "read",
      message: assistant,
    },
  };
  const entries = enrichTimelineItems([messageItem(assistant), tool], cache);
  assert(entries.length === 1, "a message echoed as both message and tool member projects one row");
}

console.log("chat enrichment cache checks passed");
