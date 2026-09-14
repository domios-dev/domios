import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import plugin from "../src/plugin";
import type { ActivityMessage } from "../src/activity";

interface Parser {
  parseSessionDelta(chunk: string): { messages: ActivityMessage[] };
}

const context = { module: { exports: {} as { default: Parser } }, exports: {}, console };
new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
const parsers = [plugin as unknown as Parser, context.module.exports.default];
const timestamp = "2026-01-02T03:04:05Z";
const event = (payload: Record<string, unknown>) => JSON.stringify({ type: "event_msg", timestamp, payload });
const completed = (item: Record<string, unknown>, turnId?: string) => event({ type: "item_completed", ...(turnId ? { turn_id: turnId } : {}), item });

const webEnd = {
  type: "web_search_end", call_id: "ws-opaque", turn_id: "turn-opaque", query: "opaque query",
  action: { type: "search", query: "opaque query" },
  results: [{ title: "opaque-title-1" }, { title: "opaque-title-2" }, { title: "opaque-title-3" }],
};
const imageEnd = {
  type: "image_generation_end", call_id: "ig-opaque", status: "completed", revised_prompt: "opaque prompt",
  result: "opaque-base64-bytes", saved_path: "C:\\opaque\\out.png",
};
const imageView = { type: "ImageView", id: "view-opaque", path: "file:///C:/opaque/shot.png" };
const extension = {
  type: "Extension", kind: "web.search", id: "ext-opaque", query: "https://example.test/page",
  action: { type: "openPage", url: "https://example.test/page" },
  results: [{ type: "text_result", title: "opaque-page", thumbnail_url: "https://images.example.test/remote.png" }],
};

for (const parser of parsers) {
  const parse = (lines: string[]) => JSON.parse(JSON.stringify(parser.parseSessionDelta(lines.join("\n")))).messages as ActivityMessage[];
  const originalWarn = console.warn;
  let messages: ActivityMessage[];
  try {
    console.warn = () => {};
    messages = parse([
      event(webEnd),
      "{not json",
      event({ type: "web_search_end", call_id: "ws-no-query" }),
      event({ type: "image_generation_end", status: "completed" }),
      event(imageEnd),
      completed(imageView, "turn-opaque"),
      completed({ type: "ImageView", id: "view-bad", path: 7 }),
      completed({ type: "Extension", kind: "web.search", id: "ext-empty" }),
      completed({ ...extension, id: "ext-other", kind: "other.tool" }),
      completed(extension, "turn-opaque"),
      event(webEnd),
    ]);
  } finally {
    console.warn = originalWarn;
  }

  assert.deepEqual(messages.map((message) => message.uuid), [
    "result-ws-opaque", "result-ig-opaque", "activity-view-opaque", "activity-ext-opaque",
  ], "valid records keep source order while malformed, unknown, and duplicate lines are skipped alone");

  const web = messages[0].blocks[0].data;
  assert.equal(messages[0].blocks.length, 1);
  assert.equal(web.id, webEnd.call_id);
  assert.equal(web.tool_kind, "web");
  assert.equal(web.tool_summary, webEnd.query);
  assert.equal(web.tool_detail, webEnd.query);
  assert.equal(web.tool_flow_id, "turn-opaque");
  assert.equal(web.tool_status, "success");
  assert.deepEqual(web.tool_rich_result, { kind: "web", query: webEnd.query, action: "search", resultCount: 3 });
  assert.ok(!JSON.stringify(messages[0]).includes("opaque-title"), "web search result content is never copied into the row");

  const generated = messages[1].blocks[0].data;
  assert.equal(generated.tool_kind, "image");
  assert.equal(generated.tool_summary, "out.png");
  assert.equal(generated.tool_detail, imageEnd.saved_path);
  assert.equal(generated.tool_flow_id, undefined, "an event without a turn id keeps its flow unknown");
  assert.deepEqual(generated.tool_rich_result, {
    kind: "image", origin: "generated", status: "completed", path: imageEnd.saved_path, prompt: imageEnd.revised_prompt,
  });
  assert.ok(!JSON.stringify(messages[1]).includes(imageEnd.result), "generated image bytes never cross the boundary");

  const viewed = messages[2].blocks[0].data;
  assert.equal(viewed.tool_kind, "image");
  assert.equal(viewed.tool_summary, "shot.png");
  assert.equal(viewed.tool_flow_id, "turn-opaque");
  assert.deepEqual(viewed.tool_rich_result, { kind: "image", origin: "viewed", path: imageView.path }, "the exact source path is preserved");

  const lookup = messages[3].blocks[0].data;
  assert.equal(lookup.tool_kind, "web");
  assert.equal(lookup.tool_summary, extension.query);
  assert.deepEqual(lookup.tool_rich_result, {
    kind: "web", query: extension.query, action: "openPage", url: extension.action.url, resultCount: 1,
  });
  assert.ok(!JSON.stringify(messages[3]).includes("images.example.test"), "remote result thumbnails are never carried");

  const { results: _results, ...uncounted } = webEnd;
  const noCount = parse([event({ ...uncounted, call_id: "ws-uncounted" })])[0].blocks[0].data;
  assert.equal("resultCount" in (noCount.tool_rich_result as Record<string, unknown>), false, "an unreported count stays absent");

  const failed = parse([event({ ...imageEnd, call_id: "ig-failed", status: "failed", saved_path: undefined })])[0].blocks[0].data;
  assert.equal(failed.tool_status, "error");
  assert.deepEqual(failed.tool_rich_result, { kind: "image", origin: "generated", status: "failed", prompt: imageEnd.revised_prompt });
}

console.log("Codex web and image result source/bundle fixtures passed");
