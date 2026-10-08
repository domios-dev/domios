import assert from "node:assert/strict";
import { documentsFromTranscript } from "./chatDocuments";

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
console.log("chatDocuments: passed");
