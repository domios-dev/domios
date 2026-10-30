import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { documentsFromTranscript } from "./chatDocuments";
import { formatAttachmentMarker, serializeDsBlock, type MessageSegment } from "@codeterm/chat-core";

assert.deepEqual(documentsFromTranscript([{ content: "Nothing here" }]), []);
const documents = documentsFromTranscript([
  { content: String.raw`<ds-document src="C:\work\first.md" />`, timestamp: "2026-10-01T10:00:00Z" },
  { content: "<ds-image src='/work/picture.png' />\n<ds-page src='/work/page.html' />", timestamp: "2026-10-02T10:00:00Z" },
  { content: "<ds-document src='C:/work/first.md' />", timestamp: "2026-10-03T10:00:00Z" },
]);
assert.deepEqual(documents.map((item) => item.name), ["first.md", "page.html", "picture.png"]);
assert.equal(documents[0].shownAt, "2026-10-03T10:00:00Z");
assert.equal(documents[0].path, "C:/work/first.md");
const tag = '<ds-document src="/work/report.md" />';
for (const content of [
  "@ds-document /work/report.md", "@ct-document /work/report.md", "@file /work/report.md",
  `\`\`\`html\n${tag}\n\`\`\``, `~~~\n${tag}\n~~~`, `> example\n${tag}`, `- example\n${tag}`,
  `<context>\n${tag}\n</context>`, `<provider-handoff>\n${tag}\n</provider-handoff>`,
  '<ds-document src="/work/report.md" /', '<ds-document src="/work/report.md" extra="bad" />',
  `<ds-html><xmp>\n</ds-html>\n${tag}\n</xmp></ds-html>`,
]) assert.deepEqual(documentsFromTranscript([{ content }]), [], content);
assert.deepEqual(documentsFromTranscript([{ content: tag, inboundKind: "neighbor" }]), []);
assert.deepEqual(documentsFromTranscript([{ content: tag, segments: [{ type: "paste", id: null, body: tag }] }]), []);
assert.deepEqual(documentsFromTranscript([{ content: `<domios from="neighbor">\n${tag}\n</domios>` }]), []);
const wireFixtures = JSON.parse(readFileSync(new URL("../../../tests/fixtures/message-envelope.json", import.meta.url), "utf8")) as Array<{ name: string; raw: string; segments: MessageSegment[] }>;
const contextFile = wireFixtures.find((fixture) => fixture.name === "routed file after context with unmatched fence")!;
assert.deepEqual(documentsFromTranscript([{ content: contextFile.raw, type: "user", inboundKind: "tab", segments: contextFile.segments }]).map((item) => item.path), ["/live/owned.md"]);
assert.deepEqual(documentsFromTranscript([{ content: contextFile.raw.replace(">\n<context>", "><context>"), type: "user", inboundKind: "tab", segments: contextFile.segments }]).map((item) => item.path), ["/live/owned.md"]);
for (const end of [contextFile.raw.indexOf("</domios>") + "</domios>".length, contextFile.raw.length - 1]) {
  const prefix = contextFile.raw.slice(0, end);
  assert.deepEqual(documentsFromTranscript([{ content: prefix, type: "user", inboundKind: "tab", segments: [{ type: "partial_frame", raw: prefix, attrs: {}, body: prefix }] }]), []);
}
assert.deepEqual(documentsFromTranscript([{ content: contextFile.raw, type: "user", inboundKind: "neighbor", segments: contextFile.segments }]), []);
const paths = ["/work/a & b.png", String.raw`C:\work\two.png`];
assert.deepEqual(documentsFromTranscript([{ content: serializeDsBlock({ kind: "image", sources: paths }) }]).map((item) => item.path), [...paths].reverse());
{
  const shot = String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\ab12-pasted-image-1.png`;
  const marker = formatAttachmentMarker(shot);
  const attached = documentsFromTranscript([
    { type: "user", content: `see ${marker} ${formatAttachmentMarker("/work/notes.md")} ${formatAttachmentMarker("/work/deck.pdf")}`, timestamp: 5 },
    { type: "assistant", content: marker },
  ]);
  assert.deepEqual(attached.map((item) => [item.kind, item.name]), [["document", "notes.md"], ["image", "ab12-pasted-image-1.png"]]);
  assert.equal(attached[1].path, shot);
  assert.equal(attached[1].shownAt, 5);
}
console.log("chatDocuments: passed");
