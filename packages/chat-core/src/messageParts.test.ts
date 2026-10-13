import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { parseMessageParts, projectMessageParts, projectTranscriptParts, splitExactSendTexts } from "./messageParts";
import type { NormalizedChatMessage } from "./types";

const frame = '<domios from="report" tab="abc" label="X">body</domios>';
const text = (content: string) => ({ kind: "text", content });
const routed = (content: string) => ({ kind: "frame", content });
assert.deepEqual(parseMessageParts(`${frame}\nand text here`), [routed(frame), text("\nand text here")]);
assert.deepEqual(parseMessageParts(`text${frame}`), [text(`text${frame}`)]);
assert.deepEqual(parseMessageParts(`${frame}\n${frame}tail`), [routed(frame), routed(frame), text("tail")]);
assert.deepEqual(parseMessageParts(`${frame}text${frame}`), [routed(frame), text(`text${frame}`)]);
for (const malformed of ['<domios from="report">unclosed', '<domios from=report>x</domios>', '<domios from="report" from="tab">x</domios>']) {
  assert.deepEqual(parseMessageParts(malformed), [text(malformed)]);
}
assert.deepEqual(parseMessageParts(""), [text("")]);
assert.deepEqual(parseMessageParts(`${frame}\n<domios from="report">unclosed`), [routed(frame), text('\n<domios from="report">unclosed')]);
const payloads = [{ content: "first", correlationId: "one" }, { content: "second", correlationId: "two" }];
assert.deepEqual(splitExactSendTexts("firstsecond trailing", payloads), [
  { ...text("first"), correlationId: "one" }, { ...text("second"), correlationId: "two" }, text(" trailing"),
]);
assert.deepEqual(splitExactSendTexts("FIRST second", [{ content: "first" }]), [text("FIRST second")]);
assert.deepEqual(splitExactSendTexts("secondfirst", payloads), [text("second"), { ...text("first"), correlationId: "one" }]);
assert.deepEqual(splitExactSendTexts(`${frame}firstsecond`, payloads), [
  routed(frame), { ...text("first"), correlationId: "one" }, { ...text("second"), correlationId: "two" },
]);
assert.deepEqual(splitExactSendTexts("firstsecondsecond", payloads).map((part) => part.content), ["first", "second", "second"]);
const row: NormalizedChatMessage = { id: "row", type: "user", content: "firstsecond", correlationId: "one" };
const projected = projectMessageParts([row], payloads, "ledger");
assert.equal(projectMessageParts([row], payloads, "ledger")[0], projected[0]);
assert.deepEqual(projected[0].messageParts?.map((part) => part.content), ["first", "second"]);
assert.equal(projectMessageParts([{ ...row, correlationId: "foreign" }], payloads, "ledger")[0].messageParts, undefined);
assert.notEqual(projectMessageParts([row], [], "changed")[0], projected[0]);

const eligible = payloads.map((payload, index) => ({ ...payload, id: String(index), sentAtMs: 1, flowAtMs: 1 }));
assert.deepEqual(projectTranscriptParts([row], eligible)[0].messageParts?.map((part) => part.content), ["first", "second"]);
assert.equal(projectTranscriptParts([row], eligible.map(({ flowAtMs, ...send }) => send))[0], row);
assert.equal(projectTranscriptParts([row], eligible)[0], projectTranscriptParts([row], eligible)[0]);

const longLedger = [...eligible, ...Array.from({ length: 126 }, (_, index) => ({
  id: `unmatched-${index}`, correlationId: `unmatched-${index}`, content: `unmatched ledger payload ${index}`, sentAtMs: 1, flowAtMs: 1,
}))];
const longRows: NormalizedChatMessage[] = Array.from({ length: 5000 }, (_, index) => ({
  id: String(index), type: "user", content: `record ${index} ${"x".repeat(4000)}firstsecond`, correlationId: "one",
}));
const start = performance.now();
const cold = projectTranscriptParts(longRows, longLedger);
const coldMs = performance.now() - start;
const warmStart = performance.now();
const warm = projectTranscriptParts(longRows, longLedger);
const warmMs = performance.now() - warmStart;
assert(warm.every((record, index) => record === cold[index]));
console.log(`messageParts: regression assertions passed; 128 ledger entries / 5000 records / ${longRows.reduce((sum, record) => sum + record.content.length, 0)} chars; cold=${coldMs.toFixed(2)}ms cached=${warmMs.toFixed(2)}ms`);
