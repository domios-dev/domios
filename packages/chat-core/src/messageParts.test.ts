import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { messageRowParts, parseMessageParts, projectMessageParts, projectTranscriptParts, splitExactSendTexts } from "./messageParts";
import { formatAttachmentMarker } from "./attachmentMarker";
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
for (const remainder of ["", "\n", " \r\n\t"]) {
  assert.deepEqual(parseMessageParts(`${frame}${remainder}`), [routed(frame)]);
  assert.deepEqual(messageRowParts([routed(frame), text(remainder)]), [routed(frame)]);
  assert.deepEqual(messageRowParts(splitExactSendTexts(`${frame}${remainder}`, [{ content: frame, correlationId: "route" }])),
    [{ ...routed(frame), correlationId: "route" }]);
}
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

const image = formatAttachmentMarker("/tmp/image.png", "image", "image1");
const secondImage = formatAttachmentMarker("/tmp/second.png", "image", "image2");
const imagePart = (content: string, paths: string[]) => ({ kind: "attachment", content,
  attachments: paths.map((path, index) => ({ kind: "image", path, label: `image${index + 1}` })) });
assert.deepEqual(parseMessageParts(`${image}caption`), [imagePart(image, ["/tmp/image.png"]), text("caption")]);
assert.deepEqual(parseMessageParts(`${image} ${secondImage}caption`), [imagePart(`${image} ${secondImage}`, ["/tmp/image.png", "/tmp/second.png"]), text("caption")]);
assert.deepEqual(parseMessageParts(`caption${image}`), [text("caption"), imagePart(image, ["/tmp/image.png"])]);
assert.deepEqual(parseMessageParts(formatAttachmentMarker("/tmp/file.pdf")), [{ kind: "attachment", content: formatAttachmentMarker("/tmp/file.pdf"), attachments: [{ kind: "file", path: "/tmp/file.pdf" }] }]);

assert.deepEqual(messageRowParts(parseMessageParts(`${image} ${secondImage}caption`)).map((part) => part.content), [`${image} ${secondImage}caption`]);
assert.deepEqual(messageRowParts(splitExactSendTexts(`${image}captionnext`, [{ content: `${image}caption`, correlationId: "one" }, { content: "next", correlationId: "two" }])).map((part) => [part.content, part.correlationId]), [[`${image}caption`, "one"], ["next", "two"]]);
assert.deepEqual(messageRowParts([text("first"), text("second")]), [text("first"), text("second")]);

{
  const route = '<domios from="tab" label="Sender" tab="abc"> <tip>opaque hint</tip></domios>';
  const own = "opaque owner input";
  const ledger = [{ content: route, correlationId: "route" }, { content: own, correlationId: "owner" }];
  const combined = { id: "combined", type: "user" as const, content: `${route}${own}`, correlationId: "route" };
  assert.deepEqual(projectMessageParts([combined], ledger, "distinct-sends")[0].messageParts, [
    { kind: "frame", content: route, correlationId: "route" },
    { kind: "text", content: own, correlationId: "owner" },
  ]);
}

{
  const owner = "opaque owner payload";
  const report = '<domios from="report" label="Worker" tab="abc">opaque report <tip>opaque instruction</tip></domios>';
  const combined = { id: "owner-then-report", type: "user" as const, content: `${owner}${report}`, correlationId: "owner-send" };
  const parts = projectMessageParts([combined], [{ content: owner, correlationId: "owner-send" }], "owner-report")[0].messageParts;
  assert.deepEqual(parts, [
    { kind: "text", content: owner, correlationId: "owner-send" },
    { kind: "frame", content: report },
  ]);
  assert.deepEqual(parseMessageParts(`${owner}${report}`), [{ kind: "text", content: `${owner}${report}` }]);
}
