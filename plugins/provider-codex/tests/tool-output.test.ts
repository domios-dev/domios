import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import plugin from "../src/plugin";
import { codexToolOutputText } from "../src/toolOutput";

const output = [
  { type: "text", text: "Script completed\nWall time 0.2 seconds\nOutput:\n" },
  { type: "input_image", image_url: "data:image/png;base64,iVBORw0KGgo=", detail: "original" },
  { type: "text", text: "opaque-command-output" },
];
assert.deepEqual(JSON.parse(codexToolOutputText(output)), output);
assert.equal(codexToolOutputText([{ type: "text", text: "a" }, { type: "text", text: "b" }]), "ab");
assert.equal(codexToolOutputText(null), "");
assert.equal(codexToolOutputText("opaque"), "opaque");

interface Parser { parseSessionDelta(chunk: string): { messages: Array<{ blocks: Array<{ kind: string; data: Record<string, unknown> }> }> } }
const context = { module: { exports: {} as { default: Parser } }, exports: {} };
new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
const chunk = JSON.stringify({ type: "response_item", payload: { type: "custom_tool_call_output", call_id: "opaque-call", output } });
for (const parser of [plugin as unknown as Parser, context.module.exports.default]) {
  const blocks = parser.parseSessionDelta(chunk).messages.flatMap((message) => message.blocks);
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, "tool_result");
  assert.deepEqual(JSON.parse(blocks[0].data.content as string), output);
}
console.log("codex tool-output: 10 preservation assertions passed");
assert.equal(codexToolOutputText({z:[{type:"text",text:"value"}],a:{z:2,a:1}}),'{'+'"a":{"a":1,"z":2},"z":[{"text":"value","type":"text"}]'+'}');
assert.equal(codexToolOutputText({a:{a:1,z:2},z:[{text:"value",type:"text"}]}),codexToolOutputText({z:[{type:"text",text:"value"}],a:{z:2,a:1}}));
console.log("codex canonical tool-output: 2 ordering assertions passed");
