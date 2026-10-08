import assert from "node:assert/strict";
import { formatAttachmentMarker } from "./attachmentMarker";
import { attachmentFileName, inlineAttachmentSrc, userMessageAttachments, userVisibleText } from "./userAttachments";

const PATH = String.raw`C:\Users\dev\AppData\Local\Temp\codeterm-chat\e8a97274-pasted-image-1791277476989.png`;
const pathImage = { id: "a", mimeType: "image/png", path: PATH };

assert.equal(userVisibleText("[1 image]", [pathImage]), "");
assert.equal(userVisibleText("[2 images]", [pathImage, pathImage]), "");
assert.equal(userVisibleText("[1 image]", []), "[1 image]");
assert.equal(userVisibleText("look at [1 image] here", [pathImage]), "look at [1 image] here");

assert.equal(attachmentFileName(pathImage), "e8a97274-pasted-image-1791277476989.png");
assert.equal(attachmentFileName({ id: "b", mimeType: "image/jpeg", path: "/tmp/shot.jpg" }), "shot.jpg");
assert.equal(attachmentFileName({ id: "c", mimeType: "image/webp", data: "AAAA" }), "image.webp");
assert.equal(attachmentFileName({ id: "d", mimeType: "image/png", url: "data:image/png;base64,AAAA" }), "image.png");

assert.equal(inlineAttachmentSrc({ id: "e", mimeType: "image/png", data: "AAAA" }), "data:image/png;base64,AAAA");
assert.equal(inlineAttachmentSrc({ id: "f", mimeType: "image/png", url: "data:image/png;base64,BBBB" }), "data:image/png;base64,BBBB");
assert.equal(inlineAttachmentSrc({ id: "g", mimeType: "image/png", url: "https://example.test/x.png" }), null);
assert.equal(inlineAttachmentSrc(pathImage), null);

{
  const native = { id: "n", mimeType: "image/png", path: PATH };
  const content = `fix this ${formatAttachmentMarker(PATH)} ${formatAttachmentMarker("/tmp/b.jpg")} ${formatAttachmentMarker("/tmp/log.txt")}`;
  const view = userMessageAttachments(content, [native]);
  assert.equal(view.text, "fix this");
  assert.deepEqual(view.images.map((image) => [image.path, image.mimeType]), [[PATH, "image/png"], ["/tmp/b.jpg", "image/jpeg"]]);
  assert.deepEqual(view.files, [{ kind: "file", path: "/tmp/log.txt" }]);
}

assert.deepEqual(userMessageAttachments("[1 image]", [pathImage]).text, "");
assert.deepEqual(userMessageAttachments("plain text"), { text: "plain text", images: [], files: [], inline: null, reply: null });

{
  const content = `compare ${formatAttachmentMarker(PATH, "image", "image1")} with\n${formatAttachmentMarker("/tmp/notes.txt", "text", "text1 · 312 lines")} ok ${formatAttachmentMarker("/tmp/chip.pdf")}`;
  const view = userMessageAttachments(content);
  assert.deepEqual(view.inline, [
    { text: "compare " },
    { attachment: { kind: "image", path: PATH, label: "image1" } },
    { text: " with\n" },
    { attachment: { kind: "text", path: "/tmp/notes.txt", label: "text1 · 312 lines" } },
    { text: " ok" },
  ], "inline capsules stay where they were typed; the chip marker leaves the text");
  assert.deepEqual(view.images, [], "an inline image renders inline, not in the strip");
  assert.deepEqual(view.files, [{ kind: "file", path: "/tmp/chip.pdf" }]);
}

{
  const view = userMessageAttachments(`${formatAttachmentMarker("/tmp/dir", "folder")}`);
  assert.deepEqual(view.files, [{ kind: "folder", path: "/tmp/dir" }], "an unlabeled folder is a chip");
  assert.equal(view.inline, null);
}

{
  const quote = formatAttachmentMarker("/tmp/quote-codex.md", "quote", "Codex · 14:05", { ref: "m7", excerpt: "Done \"now\" <ok>" });
  const view = userMessageAttachments(`${quote}\nlooks wrong ${formatAttachmentMarker("/tmp/a.txt", "text", "text1")}`);
  assert.deepEqual(view.reply, { kind: "quote", path: "/tmp/quote-codex.md", label: "Codex · 14:05", reply: "m7", excerpt: "Done \"now\" <ok>" }, "the reply round-trips its attributes");
  assert.deepEqual(view.inline, [{ text: "looks wrong " }, { attachment: { kind: "text", path: "/tmp/a.txt", label: "text1" } }], "the reply leaves the inline text");
  assert.deepEqual(view.files, []);
}

{
  const quote = formatAttachmentMarker("/tmp/q.md", "quote", "3 messages", { ref: "m1", excerpt: "" });
  const view = userMessageAttachments(quote);
  assert.equal(view.reply?.label, "3 messages");
  assert.equal(view.text, "", "a reply alone leaves no text");
  assert.equal(view.inline, null);
}

console.log("userAttachments tests passed");
