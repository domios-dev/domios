import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import type { MessageSegment, NormalizedChatMessage, TimelineItem } from "@codeterm/chat-core";
import { enrichTimelineItems } from "./chatEnrichment";

const fixtures = JSON.parse(readFileSync(new URL("../../../tests/fixtures/message-envelope.json", import.meta.url), "utf8")) as Array<{ name: string; raw: string; segments: MessageSegment[] }>;
const item = (message: NormalizedChatMessage): TimelineItem => ({ kind: "message", message });
const screenshot = fixtures.find((fixture) => fixture.name === "owner merged screenshot")!;
for (const provider of ["claude", "codex", "opencode", "plugin"]) {
  const raw: NormalizedChatMessage = { id: provider, provider, type: "user", content: screenshot.raw, segments: screenshot.segments };
  const cache = new WeakMap();
  const entries = enrichTimelineItems([item(raw)], cache);
  assert.equal(entries.length, 2);
  assert.equal(entries[0].origin, undefined);
  assert.equal(entries[0].msg.content.trim(), "какой щас план?");
  assert.deepEqual(entries[1].origin, { kind: "tab", title: "Probe", tabIdShort: "41f7661c", model: "gpt-6.1-sol" });
  assert.equal(entries[1].msg.content.trim(), "Harness preflight caught ... Product code untouched.");
  assert.equal(enrichTimelineItems([item(raw)], cache)[0], entries[0]);
  assert.equal(raw.content, screenshot.raw);
}
for (const name of ["fenced example", "unclosed tag", "indented tag is content", "tool result is content", "opening tag cut before its close"]) {
  const fixture = fixtures.find((entry) => entry.name === name)!;
  const [entry] = enrichTimelineItems([item({ id: name, type: "user", content: fixture.raw, segments: fixture.segments })], new WeakMap());
  assert.equal(entry.origin, undefined);
  assert.equal(entry.msg.content, fixture.raw);
}
{
  const raw: NormalizedChatMessage = { id: "plugin", type: "user", content: "opaque", segments: [{ type: "text", text: "opaque" }], fromPlugin: "plugin-id", fromLabel: "Plugin", inboundKind: "plugin" };
  const [entry] = enrichTimelineItems([item(raw)], new WeakMap());
  assert.equal(entry.msg.fromPlugin, "plugin-id");
  assert.equal(entry.msg.inboundKind, "plugin");
}
{
  const before: NormalizedChatMessage = { id: "stream", type: "assistant", content: "a" };
  const after = { ...before, content: "ab" };
  const entries = enrichTimelineItems([item(before), item(after)], new WeakMap());
  assert.equal(entries.length, 1);
  assert.equal(entries[0].msg, after);
}
{
  const raw: NormalizedChatMessage = { id: "no-trust", type: "user", content: '<domios from="tab" tab="a">literal</domios>' };
  assert.equal(enrichTimelineItems([item(raw)], new WeakMap())[0].origin, undefined);
}
{
  // The daemon does not know the neighbor frame kind and sends it as an unknown frame, as on 83766's Codex concierge.
  const body = "Friend: \"QA Peer\"\nFriendship: f1\nMessage: m2\nKind: \"answer\"\nIn reply to: m1\nReply through the encrypted Neighbor channel.\n\nR161-PUBLIC-CONTROL-B";
  const attrs = { from: "neighbor", friendship: "f1", label: "uid", tab: "" };
  const raw: NormalizedChatMessage = { id: "neighbor", type: "user", content: `<domios from="neighbor" friendship="f1" label="uid" tab="">${body}</domios>`,
    segments: [{ type: "unknown_frame", raw: "", attrs, body }] };
  const [entry] = enrichTimelineItems([item(raw)], new WeakMap());
  assert.equal(entry.origin?.kind, "neighbor");
  assert.equal(entry.origin?.kind === "neighbor" ? entry.origin.friend : null, "QA Peer");
  assert.equal(entry.msg.content, "R161-PUBLIC-CONTROL-B");
  assert.deepEqual(entry.msg.segments, [{ type: "text", text: "R161-PUBLIC-CONTROL-B" }], "the bubble shows only the friend's words");
}
{
  // Since 29d29865b7 the daemon parses the routed envelope as a "neighbor" frame, with its own text segments and the tip apart.
  const attrs = { from: "neighbor", friend: "QA Peer", friendship: "f1", message: "m3", kind: "question" };
  const raw: NormalizedChatMessage = { id: "neighbor-frame", type: "user", content: "<domios from=\"neighbor\">…</domios>",
    segments: [{ type: "frame", kind: "neighbor", from: "neighbor", attrs, body: "Is 2 &lt; 3?", tip: "Reply through domios account neighbors send.",
      segments: [{ type: "text", text: "Is 2 &lt; 3?" }] }] };
  const [entry] = enrichTimelineItems([item(raw)], new WeakMap());
  assert.deepEqual(entry.origin, { kind: "neighbor", friendshipId: "f1", message: "m3", friend: "QA Peer", messageKind: "question",
    instruction: "Reply through domios account neighbors send." });
  assert.deepEqual(entry.msg.segments, [{ type: "text", text: "Is 2 < 3?" }], "the bubble shows the friend's words as written");
}
console.log("envelope enrichment: 13 cases passed");
