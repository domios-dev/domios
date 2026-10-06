import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import plugin from "../src/plugin";
import { parseCodexUserContent } from "../src/userContent";

interface RichBlock { kind: string; data: Record<string, any> }
interface Parser {
  parseSessionDelta(chunk: string): { messages: Array<{ role: string; blocks: RichBlock[] }> };
}

const context = { module: { exports: {} as { default: Parser } }, exports: {}, console };
new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
const parsers = [plugin as unknown as Parser, context.module.exports.default];

const PATH = "C:\\Users\\User\\AppData\\Local\\Temp\\codeterm-chat\\e8a97274-pasted-image-1791277476989.png";
const OPEN = `<image name=[Image #1] path="${PATH}">`;
const recorded = [
  { type: "input_text", text: OPEN },
  { type: "input_image", image_url: "data:image/png;base64,iVBORw0KGgo=" },
  { type: "input_text", text: "</image>" },
  { type: "input_text", text: "[Image #1]" },
];

{
  const parsed = parseCodexUserContent(recorded);
  assert.equal(parsed.text, "");
  assert.deepEqual(parsed.images, [{ name: "[Image #1]", path: PATH, mediaType: "image/png", data: null }]);
}

{
  const parsed = parseCodexUserContent(`${OPEN}</image>[Image #1]`);
  assert.equal(parsed.text, "");
  assert.equal(parsed.images.length, 1);
  assert.equal(parsed.images[0].path, PATH);
}

{
  const parsed = parseCodexUserContent([...recorded.slice(0, 3), { type: "input_text", text: "what is wrong here [Image #1]" }]);
  assert.equal(parsed.text, "what is wrong here");
  assert.equal(parsed.images.length, 1);
}

{
  const parsed = parseCodexUserContent([
    { type: "input_text", text: "<image name=[Image #1]>" },
    { type: "input_image", image_url: "data:image/jpeg;base64,QUJD" },
    { type: "input_text", text: "</image>" },
    { type: "input_text", text: "compare [Image #1] with [Image #2]" },
  ]);
  assert.equal(parsed.text, "compare with [Image #2]");
  assert.deepEqual(parsed.images, [{ name: "[Image #1]", path: null, mediaType: "image/jpeg", data: "QUJD" }]);
}

{
  const parsed = parseCodexUserContent([{ type: "input_image", image_url: "data:image/webp;base64,V0VCUA==" }]);
  assert.deepEqual(parsed.images, [{ name: null, path: null, mediaType: "image/webp", data: "V0VCUA==" }]);
}

{
  const plain = "  keep <b>markup</b> and [Image #9] exactly  ";
  assert.deepEqual(parseCodexUserContent([{ type: "input_text", text: plain }]), { text: plain, images: [] });
  assert.deepEqual(parseCodexUserContent(plain), { text: plain, images: [] });
}

const line = JSON.stringify({
  timestamp: "2026-10-06T09:04:38.306Z",
  type: "response_item",
  payload: { type: "message", role: "user", content: recorded },
});
const captioned = JSON.stringify({
  type: "response_item",
  payload: {
    type: "message",
    role: "user",
    content: [...recorded.slice(0, 3), { type: "input_text", text: "look [Image #1]" }],
  },
});

for (const parser of parsers) {
  const [imageOnly, withText] = JSON.parse(JSON.stringify(parser.parseSessionDelta(`${line}\n${captioned}\n`).messages));
  assert.equal(imageOnly.role, "user");
  assert.deepEqual(imageOnly.blocks, [{ kind: "image", data: { source: { media_type: "image/png", path: PATH } } }]);
  assert.deepEqual(withText.blocks.map((block) => block.kind), ["text", "image"]);
  assert.equal(withText.blocks[0].data.text, "look");
}

console.log("codex user image tests passed");
