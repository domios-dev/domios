import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { envelopeRows, envelopeText, type MessageSegment } from "./envelope";
import { matchedOptimisticEchoes } from "./dedupe";
import { markVoice } from "./markers";
import { formatAttachmentMarker } from "./attachmentMarker";

const fixtures = JSON.parse(readFileSync(new URL("../../../tests/fixtures/message-envelope.json", import.meta.url), "utf8")) as Array<{ name: string; raw: string; segments: MessageSegment[] }>;
const expectedRows: Record<string, number> = {
  "owner merged screenshot": 2, "voice transcript echo": 1, paste: 1, attachment: 1,
  "two frames in one record": 2, "fenced example": 1, "unclosed tag": 1,
  "inline frame has no provenance": 1, "tool result is content": 1, "routed paste wrapper": 1,
  "inline code attachment is text": 1, "unicode and crlf markers": 3, empty: 0,
  "indented tag is content": 1, "malformed attributes": 1, "routed attachment body": 1, "frame body quoted example": 1, "glued routed then owner": 2, "owner newline routed": 2,
};
for (const fixture of fixtures) {
  const rows = envelopeRows(fixture.segments);
  assert.equal(rows.length, expectedRows[fixture.name], fixture.name);
}
const screenshot = fixtures.find((fixture) => fixture.name === "owner merged screenshot")!;
assert.deepEqual(envelopeRows(screenshot.segments).map((row) => [row.frame?.attrs.tab, row.content.trim()]), [
  [undefined, "какой щас план?"], ["41f7661c", "Harness preflight caught ... Product code untouched."],
]);
const voice = fixtures.find((fixture) => fixture.name === "voice transcript echo")!;
assert.equal(envelopeText(voice.segments), "привет <opaque> & world");
const echo = { id: "echo", content: markVoice("/tmp/voice.webm", null, "opaque"), sentAtMs: 10_000 };
const candidate = { id: "source", type: "user" as const, content: voice.raw, segments: voice.segments, timestamp: new Date(0).toISOString() };
assert.deepEqual(matchedOptimisticEchoes([echo], [candidate]).matchedIds, ["echo"]);
assert.deepEqual(matchedOptimisticEchoes([echo], [{ ...candidate, segments: [{ type: "attachment", kind: "voice", path: "/tmp/another.webm", label: "voice", body: "opaque", status: null }] }]).matchedIds, []);
const header: MessageSegment = { type: "frame", from: "system", attrs: { from: "system" }, body: "", tip: null };
assert.deepEqual(envelopeRows([header, { type: "text", text: "opaque trailing paste" }]).map((row) => row.content), ["opaque trailing paste"]);
assert.equal(envelopeRows([header, { type: "text", text: "separate send", correlationId: "owner" }]).length, 2);
const captioned = `caption ${formatAttachmentMarker("/tmp/image.png", "image")}`;
assert.deepEqual(matchedOptimisticEchoes([{ id: "image", content: captioned, sentAtMs: 0 }], [{
  id: "image-source", type: "user", content: captioned,
  segments: [{ type: "text", text: "caption " }, { type: "attachment", kind: "image", path: "/tmp/image.png", label: null, body: null, status: null }],
}]).matchedIds, ["image"]);
console.log(`envelope parity: ${fixtures.length} shared fixtures and 7 projection/echo checks passed`);
