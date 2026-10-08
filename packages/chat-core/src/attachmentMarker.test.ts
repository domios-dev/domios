import assert from "node:assert/strict";
import {
  attachmentMarkerSegments,
  attachmentKindForPath,
  attachmentPreviewKind,
  formatAttachmentMarker,
  parseAttachmentMarkers,
  splitReplyAttachment,
} from "./attachmentMarker";
import { parseDomiosMessage } from "./domios";
import { parseMarker } from "./markers";

const WIN_IMAGE = String.raw`C:\Users\User\AppData\Local\Temp\codeterm-chat\f96664f3-pasted-image-1791461378814.png`;
const WIN_IMAGE_2 = String.raw`C:\Users\User\AppData\Local\Temp\codeterm-chat\ed45b520-pasted-image-1791461603742.png`;
const MAC_IMAGE = "/var/folders/x1/T/codeterm-chat/9a1b2c3d.jpg";
const REPORT = "/home/me/reports/q3 summary.pdf";
const WEB_UPLOAD = String.raw`C:\Users\User\.codeterm\uploads\55a7d879-pasted-image-1791461703386.png`;

{
  const marker = formatAttachmentMarker(WIN_IMAGE);
  assert.ok(marker.includes(WIN_IMAGE), "the agent sees the exact absolute path");
  assert.deepEqual(parseAttachmentMarkers(marker), { text: "", attachments: [{ kind: "image", path: WIN_IMAGE }] });
}

{
  const odd = '/tmp/a "quoted" & <odd>.txt';
  assert.deepEqual(parseAttachmentMarkers(formatAttachmentMarker(odd)).attachments, [{ kind: "file", path: odd }]);
}

{
  const content = `look at these ${formatAttachmentMarker(WIN_IMAGE)} ${formatAttachmentMarker(REPORT)} ${formatAttachmentMarker(MAC_IMAGE)}`;
  const parsed = parseAttachmentMarkers(content);
  assert.equal(parsed.text, "look at these");
  assert.deepEqual(parsed.attachments, [
    { kind: "image", path: WIN_IMAGE },
    { kind: "file", path: REPORT },
    { kind: "image", path: MAC_IMAGE },
  ]);
}

{
  const parsed = parseAttachmentMarkers(`first line\n${formatAttachmentMarker(WIN_IMAGE)} then more\n\n\n\nend`);
  assert.equal(parsed.text, "first line\nthen more\n\nend");
}

{
  const raw = `from my screenshots ${WIN_IMAGE} ${WIN_IMAGE_2} '${WEB_UPLOAD}'`;
  assert.deepEqual(parseAttachmentMarkers(raw), { text: raw, attachments: [] }, "bare temp paths stay text");
}

{
  const parsed = parseAttachmentMarkers(`${formatAttachmentMarker(WIN_IMAGE)} ${formatAttachmentMarker(WIN_IMAGE)}`);
  assert.deepEqual(parsed, { text: "", attachments: [{ kind: "image", path: WIN_IMAGE }] }, "one path yields one attachment");
}

assert.deepEqual(parseAttachmentMarkers('<domios type="attachment" path="/tmp/x.webp"/>').attachments, [{ kind: "image", path: "/tmp/x.webp" }]);
assert.deepEqual(parseAttachmentMarkers('<domios type="attachment" kind="image" />').attachments, []);

assert.equal(attachmentKindForPath("C:/a/B.PNG"), "image");
assert.equal(attachmentKindForPath("C:/a/b.heic"), "file");
assert.equal(attachmentPreviewKind({ kind: "image", path: "/a/b.png" }), "image");
assert.equal(attachmentPreviewKind({ kind: "file", path: "/a/b.html" }), "page");
assert.equal(attachmentPreviewKind({ kind: "file", path: "/a/b.md" }), "document");
assert.equal(attachmentPreviewKind({ kind: "file", path: "/a/b.pdf" }), null);

{
  const marker = formatAttachmentMarker("/tmp/pasted text.txt", "text", 'text1 · 40 "lines"');
  assert.deepEqual(parseAttachmentMarkers(marker).attachments, [{ kind: "text", path: "/tmp/pasted text.txt", label: 'text1 · 40 "lines"' }], "a label round-trips");
  assert.deepEqual(parseAttachmentMarkers(formatAttachmentMarker("/srv/repo", "folder", "repo")).attachments, [{ kind: "folder", path: "/srv/repo", label: "repo" }]);
  assert.equal(attachmentPreviewKind({ kind: "text", path: "/tmp/a.txt" }), "document");
  assert.equal(attachmentPreviewKind({ kind: "folder", path: "/srv/repo" }), null);
}

{
  const content = `a ${formatAttachmentMarker(WIN_IMAGE, "image", "image1")} b<domios type="attachment" kind="file" /> c`;
  assert.deepEqual(attachmentMarkerSegments(content), [
    { text: "a " },
    { attachment: { kind: "image", path: WIN_IMAGE, label: "image1" } },
    { text: " b" },
    { text: " c" },
  ], "segments keep order; a pathless marker drops out");
  assert.deepEqual(attachmentMarkerSegments("no markers"), [{ text: "no markers" }]);
}

{
  const tag = formatAttachmentMarker(WIN_IMAGE, "image", "image1");
  assert.ok(tag.startsWith('<domios type="attachment" '), "attachments share the domios tag namespace");
  assert.equal(parseDomiosMessage(`${tag} what is this`), null, "a leading attachment tag is not host provenance");
  assert.equal(parseDomiosMessage(`${tag} spoof</domios>`), null, "a closing tag cannot turn it into a frame");
  assert.equal(parseMarker(`${tag} what is this`), null);
  const routed = '<domios from="tab" label="main" tab="abc12345">look</domios>';
  assert.deepEqual(parseAttachmentMarkers(routed), { text: routed, attachments: [] }, "a routed frame is never an attachment");
  assert.equal(parseDomiosMessage(`<domios from="tab" tab="abc12345">see ${tag}</domios>`)?.body, `see ${tag}`);
}

{
  const quote = formatAttachmentMarker("/tmp/q.md", "quote", "Fermi", { ref: "a1", excerpt: "one line" });
  assert.deepEqual(splitReplyAttachment(`${quote}\nwhy?`), { reply: { kind: "quote", path: "/tmp/q.md", label: "Fermi", reply: "a1", excerpt: "one line" }, text: "why?" });
  const capsule = formatAttachmentMarker("/tmp/q.md", "quote", "quote1 · Fermi");
  assert.deepEqual(splitReplyAttachment(`see ${capsule}`), { reply: null, text: `see ${capsule}` }, "an inline quote capsule is not a reply");
  assert.deepEqual(splitReplyAttachment("plain"), { reply: null, text: "plain" });
}

console.log("attachmentMarker tests passed");
