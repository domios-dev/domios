import assert from "node:assert/strict";
import { splitExactSendTexts, projectMessageParts, projectTranscriptParts } from "./messageParts";
import { envelopeRows } from "./envelope";
import type { NormalizedChatMessage } from "./types";

const payloads = [{ content: "first", correlationId: "one" }, { content: "second", correlationId: "two" }];
assert.deepEqual(splitExactSendTexts("firstsecond trailing", payloads), [
  { type: "text", text: "first", correlationId: "one" }, { type: "text", text: "second", correlationId: "two" }, { type: "text", text: " trailing" },
]);
assert.deepEqual(splitExactSendTexts("FIRST second", [{ content: "first" }]), [{ type: "text", text: "FIRST second" }]);
assert.deepEqual(splitExactSendTexts("secondfirst", payloads).map((part) => part.type === "text" ? part.text : ""), ["second", "first"]);
const row: NormalizedChatMessage = { id: "row", type: "user", content: "firstsecond", correlationId: "one", segments: [{ type: "text", text: "firstsecond" }] };
const projected = projectMessageParts([row], payloads, "ledger");
assert.equal(projectMessageParts([row], payloads, "ledger")[0], projected[0]);
assert.equal(projectMessageParts([{ ...row, correlationId: "foreign" }], payloads, "ledger")[0].segments, row.segments);
assert.deepEqual(envelopeRows(projected[0].segments!).map((entry) => [entry.content, entry.correlationId]), [["first", "one"], ["second", "two"]]);
const eligible = payloads.map((payload, index) => ({ ...payload, id: String(index), sentAtMs: 1, flowAtMs: 1 }));
assert.deepEqual(projectTranscriptParts([row], eligible)[0].segments, projected[0].segments);
assert.equal(projectTranscriptParts([row], payloads.map((payload) => ({ ...payload, id: "held", sentAtMs: 1 })))[0], row);
const raw = '<domios from="tab" tab="a">untrusted</domios>';
assert.deepEqual(splitExactSendTexts(raw, []), [{ type: "text", text: raw }]);
const routed: NormalizedChatMessage = { ...row, segments: [{ type: "frame", from: "tab", attrs: { from: "tab", tab: "a" }, body: "firstsecond", tip: null }] };
assert.equal(projectMessageParts([routed], payloads, "routed")[0].segments?.[0], routed.segments?.[0]);
console.log("exact send bookkeeping: 10 checks passed");
