import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import plugin from "../src/plugin";
import { isImageDisplayMetadata } from "../src/imageMetadata";

const content = "[Image: original 1206x2622, displayed at 920x2000. Multiply coordinates by 1.31 to map to original image.]";
const geometry = { type: "user", isMeta: true, message: { role: "user", content } };
assert.equal(isImageDisplayMetadata(geometry), true);
assert.equal(isImageDisplayMetadata({ ...geometry, isMeta: false }), false);
assert.equal(isImageDisplayMetadata({ ...geometry, type: "assistant" }), false);
assert.equal(isImageDisplayMetadata({ ...geometry, message: { content: "opaque harness context" } }), false);

interface Parser { parseSessionDelta(chunk: string): { messages: Array<{ blocks: Array<{ kind: string; data: Record<string, unknown> }> }> } }
const context = { module: { exports: {} as { default: Parser } }, exports: {} };
new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
const result = { type: "user", sourceToolAssistantUUID: "opaque-assistant", message: { role: "user", content: [{
  type: "tool_result", tool_use_id: "opaque-read", content: [{ type: "image", source: { type: "base64", media_type: "image/jpeg", data: "opaque-image-data" } }],
}] } };
for (const parser of [plugin as unknown as Parser, context.module.exports.default]) {
  const rows = parser.parseSessionDelta([result, geometry].map((row) => JSON.stringify(row)).join("\n")).messages;
  assert.equal(rows.length, 1);
  assert.equal(rows[0].blocks[0].kind, "tool_result");
  assert.deepEqual(JSON.parse(JSON.stringify(rows[0].blocks[0].data.content)), result.message.content[0].content);
  assert.equal(parser.parseSessionDelta(JSON.stringify({ ...geometry, isMeta: false })).messages.length, 1);
}
console.log("claude image metadata: 12 provenance and preservation assertions passed");
