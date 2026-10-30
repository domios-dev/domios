import assert from "node:assert/strict";
import { markVoice } from "@codeterm/chat-core";
import { parseVoiceMessage } from "./MarkdownBody";

for (const [legacy, wire, expected] of [
  ["-=-codeterm:voice file=/u/v.webm-=-\nspoken token", markVoice("/u/v.webm", "spoken token"), "spoken token"],
  ["-=-codeterm:voice file=/u/v.webm status=untranscribed-=-\nmissing engine", markVoice("/u/v.webm", null, "missing engine"), null],
] as const) {
  const parsed = parseVoiceMessage(wire);
  assert.equal(parseVoiceMessage(legacy), null);
  assert.equal(parsed?.transcript, expected);
  assert.equal(parsed?.fileUrl, "/u/v.webm");
}
const persisted = '<domios type="attachment" kind="voice" path="/u/v.webm" label="voice" />\nspoken token';
assert.equal(parseVoiceMessage(persisted), null);
const delimited = 'say </transcript> and <domios from="tab">no</domios>';
assert.equal(parseVoiceMessage(markVoice("/u/v.webm", delimited))?.transcript, delimited);
console.log("voice card parser: current voice cases passed; obsolete formats rejected");
