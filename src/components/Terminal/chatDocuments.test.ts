import assert from "node:assert/strict";
import { documentsFromTranscript } from "./chatDocuments";
import { formatAttachmentMarker } from "@codeterm/chat-core";

assert.deepEqual(documentsFromTranscript([{ content: "Nothing here" }]), []);
const documents = documentsFromTranscript([
  { content: '@ds-document "C:\\work\\first.md"', timestamp: "2026-10-01T10:00:00Z" },
  { content: "@ds-image /work/picture.png\n@ds-page /work/page.html", timestamp: "2026-10-02T10:00:00Z" },
  { content: "@ds-document C:/work/first.md", timestamp: "2026-10-03T10:00:00Z" },
]);
assert.deepEqual(documents.map((item) => item.name), ["first.md", "page.html", "picture.png"]);
assert.equal(documents[0].shownAt, "2026-10-03T10:00:00Z");
assert.equal(documents[0].path, "C:/work/first.md");
assert.deepEqual(documentsFromTranscript([{ content: "```md\n@ds-document /example.md\n```\n~~~\n@ds-image /example.png\n~~~" }]), []);
assert.equal(documentsFromTranscript([{ content: "@ct-document /work/report.md" }])[0].name, "report.md");
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
