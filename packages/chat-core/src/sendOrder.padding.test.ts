import assert from "node:assert/strict";
import { reconcileSends } from "./sendOrder";
import { formatAttachmentMarker } from "./attachmentMarker";
import { userEchoBody } from "./userEchoBody";

const sentAtMs = 1792057536561;
const chip = formatAttachmentMarker("C:/Temp/image.png", "image", "image1");
const variants = [
  ["hello", "\n\nhello\n\n"],
  ["hello", "<pasted_content>hello</pasted_content>"],
  ["hello", "\n\nhello\n\npasted by Domios\n"],
  ["hello\nworld", "\r\nhello\r\nworld\r\n"],
  ["hello", '<pasted_content id="a">\n\nhello\n</pasted_content id="a">\n\npasted by Domios'],
  [`\n\n${chip} hello`, `<pasted_content id="a">\r\n${chip} hello\r\n</pasted_content>\r\npasted by Domios`],
];
for (const [content, echo] of variants) {
  assert.equal(userEchoBody(echo), userEchoBody(content));
  const result = reconcileSends([{ id: "send", content, sentAtMs }], [
    { id: "turn", type: "user", content: echo, timestamp: String(sentAtMs + 1) },
  ]);
  assert.equal(result.pending.length, 0, "only the transcript bubble remains");
  assert.equal(result.echoes.size, 1);
}
const different = reconcileSends([{ id: "send", content: "approve", sentAtMs }], [
  { id: "turn", type: "user", content: "reject", timestamp: String(sentAtMs + 1) },
]);
assert.equal(different.pending.length, 1);
assert.equal(different.echoes.size, 0);
const literal = '<pasted_content>hello</pasted_content> unrelated tail';
assert.equal(userEchoBody(literal), literal);
assert.equal(userEchoBody("pasted by Domios"), "pasted by Domios");
console.log("padded paste reconciliation: 22 assertions passed");
