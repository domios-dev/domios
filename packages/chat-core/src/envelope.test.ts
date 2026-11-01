import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { envelopeRows, envelopeText, type MessageSegment } from "./envelope";
import { matchedOptimisticEchoes } from "./dedupe";
import { markVoice } from "./markers";
import { formatAttachmentMarker } from "./attachmentMarker";
import type { FrameKind } from "./gen/FrameKind";
import { parseDsBlocks } from "./dsBlocks";

interface Fixture { name: string; source: string; raw: string; route: string; quoted?: boolean; segments: MessageSegment[] }
const fixtures = JSON.parse(readFileSync(new URL("../../../tests/fixtures/message-envelope.json", import.meta.url), "utf8")) as Fixture[];
const hostMarkup = ["<domios", "<tip>", "pasted by Domios", "<pasted_content"];
const frameKinds: Record<FrameKind, true> = {
  tab: true, report: true, system: true, reminder: true, mesh: true, telegram: true, pebble: true,
  plugin: true, plugin_config: true, system_prompt: true, voice: true, neighbor: true,
};
const expectedRows: Record<string, number> = {
  "system header with trailing Windows path": 1, "owner merged screenshot": 2, "voice transcript echo": 1, paste: 1, attachment: 1,
  "two frames in one record": 2, "fenced example": 1, "unclosed tag": 1, "inline frame after owner text": 2,
  "tool result is content": 1, "routed paste wrapper": 1, "inline code attachment is text": 1, "unicode and crlf markers": 3,
  empty: 0, "indented tag is content": 1, "malformed attributes": 1, "routed attachment body": 1, "frame body quoted example": 1,
  "glued routed then owner": 2, "owner newline routed": 2, "codex app-server ordinal 527": 1,
  "codex ordinal 527 with trailing newline": 1, "codex ordinal 527 truncated inside the tip": 1, "claude paste queue-operation": 1,
  "claude paste queue-operation with CRLF": 1, "report frame": 1, "system frame with context": 1, "notification answer": 1,
  "recurring reminder frame": 1, "mesh frame with peer": 1, "three glued frames": 3, "owner text glued to a report frame": 2,
  "pad-substituted body": 1, "owner text with trailing pasted marker": 1, "pasted voice note": 1, "unknown sender kind": 1,
  "opening tag cut before its close": 1, "neighbor question with host tip": 1,
  "routed file after context with unmatched fence": 1,
};

function displayed(segments: readonly MessageSegment[]): string[] {
  return segments.flatMap((segment) => {
    if (segment.type === "text") return [segment.text];
    if (segment.type === "frame") {
      const nested = segment.segments ?? [];
      return [...(nested.length ? displayed(nested) : [segment.body]), ...(segment.tip ? [segment.tip] : [])];
    }
    if (segment.type === "attachment") return [segment.body ?? "", segment.label ?? ""];
    return [segment.body];
  });
}

for (const fixture of fixtures) {
  assert.ok(fixture.source, `${fixture.name} names its source`);
  const rows = envelopeRows(fixture.segments);
  assert.equal(rows.length, expectedRows[fixture.name], fixture.name);
  for (const segment of fixture.segments) {
    if (segment.type === "frame") assert.ok(frameKinds[segment.kind], `${fixture.name}: ${segment.kind}`);
  }
  if (fixture.quoted) continue;
  for (const text of [...displayed(fixture.segments), ...rows.flatMap((row) => displayed(row.segments))]) {
    for (const markup of hostMarkup) assert.ok(!text.includes(markup), `${fixture.name}: ${markup} displayed in ${JSON.stringify(text)}`);
  }
}
const screenshot = fixtures.find((fixture) => fixture.name === "owner merged screenshot")!;
assert.deepEqual(envelopeRows(screenshot.segments).map((row) => [row.frame?.attrs.tab, row.content.trim()]), [
  [undefined, "какой щас план?"], ["41f7661c", "Harness preflight caught ... Product code untouched."],
]);
const codex = fixtures.find((fixture) => fixture.name === "codex ordinal 527 with trailing newline")!;
assert.deepEqual(envelopeRows(codex.segments).map((row) => [row.frame?.kind, row.frame?.tip != null]), [["tab", true]]);
const voice = fixtures.find((fixture) => fixture.name === "voice transcript echo")!;
const contextFile = fixtures.find((fixture) => fixture.name === "routed file after context with unmatched fence")!;
const contextRows = envelopeRows(contextFile.segments);
assert(contextFile.segments[0].type === "frame");
assert.equal(contextRows[0].frame?.kind, "tab");
assert.equal(contextRows[0].content, contextFile.segments[0].body);
assert.deepEqual(parseDsBlocks(contextRows[0].content).flatMap((segment) => segment.type === "block" && segment.block.kind === "document" ? [segment.block.src] : []), ["/live/owned.md"]);
assert.equal(envelopeText(voice.segments), "привет <opaque> & world");
const echo = { id: "echo", content: markVoice("/tmp/voice.webm", null, "opaque"), sentAtMs: 10_000 };
const candidate = { id: "source", type: "user" as const, content: voice.raw, segments: voice.segments, timestamp: new Date(0).toISOString() };
assert.deepEqual(matchedOptimisticEchoes([echo], [candidate]).matchedIds, ["echo"]);
assert.deepEqual(matchedOptimisticEchoes([echo], [{ ...candidate, segments: [{ type: "attachment", kind: "voice", path: "/tmp/another.webm", label: "voice", body: "opaque", status: null }] }]).matchedIds, []);
const header: MessageSegment = { type: "frame", kind: "system", from: "system", attrs: { from: "system" }, body: "", tip: null };
assert.deepEqual(envelopeRows([header, { type: "text", text: "opaque trailing paste" }]).map((row) => row.content), ["opaque trailing paste"]);
assert.equal(envelopeRows([header, { type: "text", text: "separate send", correlationId: "owner" }]).length, 2);
const captioned = `caption ${formatAttachmentMarker("/tmp/image.png", "image")}`;
assert.deepEqual(matchedOptimisticEchoes([{ id: "image", content: captioned, sentAtMs: 0 }], [{
  id: "image-source", type: "user", content: captioned,
  segments: [{ type: "text", text: "caption " }, { type: "attachment", kind: "image", path: "/tmp/image.png", label: null, body: null, status: null }],
}]).matchedIds, ["image"]);
console.log(`envelope parity: ${fixtures.length} shared fixtures and context/projection/echo checks passed`);
