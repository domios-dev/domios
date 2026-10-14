import assert from "node:assert/strict";
import { markVoice } from "@codeterm/chat-core";
import { parseVoiceMessage } from "./MarkdownBody";

for (const [legacy, wire, expected] of [
  ["-=-codeterm:voice file=/u/v.webm-=-\nspoken token", markVoice("/u/v.webm", "spoken token"), "spoken token"],
  ["-=-codeterm:voice file=/u/v.webm status=untranscribed-=-\nmissing engine", markVoice("/u/v.webm", null, "missing engine"), null],
] as const) {
  const parsed = parseVoiceMessage(wire);
  assert.deepEqual(parsed, parseVoiceMessage(legacy));
  assert.equal(parsed?.transcript, expected);
  assert.equal(parsed?.fileUrl, "/u/v.webm");
}
console.log("voice card parser: 2 parity cases passed");
