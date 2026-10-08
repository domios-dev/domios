import assert from "node:assert/strict";
import {
  attachmentKindForPath,
  attachmentPreviewKind,
  formatAttachmentMarker,
  parseAttachmentMarkers,
} from "./attachmentMarker";

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
  const legacy = `смотри надо отправить это\n\nиз моих скринов ${WIN_IMAGE} ${WIN_IMAGE_2} ${MAC_IMAGE}`;
  const parsed = parseAttachmentMarkers(legacy);
  assert.equal(parsed.text, "смотри надо отправить это\n\nиз моих скринов");
  assert.deepEqual(parsed.attachments.map((item) => item.path), [WIN_IMAGE, WIN_IMAGE_2, MAC_IMAGE]);
}

{
  const quoted = String.raw`'C:\Users\John Doe\AppData\Local\Temp\codeterm-chat\abc-notes.md'`;
  const parsed = parseAttachmentMarkers(`see ${quoted} and ${WEB_UPLOAD}`);
  assert.equal(parsed.text, "see and");
  assert.deepEqual(parsed.attachments, [
    { kind: "file", path: String.raw`C:\Users\John Doe\AppData\Local\Temp\codeterm-chat\abc-notes.md` },
    { kind: "image", path: WEB_UPLOAD },
  ]);
}

{
  const untouched = String.raw`keep C:\repo\shot.png and https://x.test/codeterm-chat/a.png and xC:\Temp\codeterm-chat\a.png`;
  assert.deepEqual(parseAttachmentMarkers(untouched), { text: untouched, attachments: [] });
}

{
  const voice = String.raw`[voice C:\Users\me\.codeterm\uploads\1a2b-voice-note-1.webm: hello there]`;
  assert.deepEqual(parseAttachmentMarkers(`say ${voice}`), { text: `say ${voice}`, attachments: [] }, "voice references stay text");
}

{
  const parsed = parseAttachmentMarkers(`${formatAttachmentMarker(WIN_IMAGE)} ${WIN_IMAGE}`);
  assert.equal(parsed.attachments.length, 1, "one path yields one attachment");
}

assert.deepEqual(parseAttachmentMarkers('<ds-attachment path="/tmp/x.webp"/>').attachments, [{ kind: "image", path: "/tmp/x.webp" }]);
assert.deepEqual(parseAttachmentMarkers('<ds-attachment kind="image" />').attachments, []);

assert.equal(attachmentKindForPath("C:/a/B.PNG"), "image");
assert.equal(attachmentKindForPath("C:/a/b.heic"), "file");
assert.equal(attachmentPreviewKind({ kind: "image", path: "/a/b.png" }), "image");
assert.equal(attachmentPreviewKind({ kind: "file", path: "/a/b.html" }), "page");
assert.equal(attachmentPreviewKind({ kind: "file", path: "/a/b.md" }), "document");
assert.equal(attachmentPreviewKind({ kind: "file", path: "/a/b.pdf" }), null);

console.log("attachmentMarker tests passed");
