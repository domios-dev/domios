import assert from "node:assert/strict";
import { test } from "node:test";
import { formatAttachmentMarker as marker } from "./attachmentMarker";
import { modelImageEchoMatches } from "./attachmentEcho";

const original = [marker("C:/chat/phone.heic", "file", "image1"), marker("C:/chat/clip.mov", "video", "video1"),
  marker("C:/chat/document.pdf", "file", "document.pdf"), marker("C:/chat/unknown.payload", "file", "unknown.payload")];
const pair = [marker("C:/chat/phone.heic", "file", "original"), marker("C:/pad/inbox/opaque-model.jpg", "image", "image1")];
const before = `Inspect these: ${original.join(" ")}`;
const after = `Inspect these: ${[...pair, ...original.slice(1)].join(" ")}`;

test("host model copies keep the original multi-attachment identity through paste wrappers", () => {
  assert.equal(modelImageEchoMatches(before, after), true);
  assert.equal(modelImageEchoMatches(before, `<pasted_content>\n${after}\npasted by Domios\n</pasted_content>`), true);
  assert.equal(modelImageEchoMatches(before.replace("C:/", "C:\\"), after), true);
  assert.equal(modelImageEchoMatches(before.replace('kind="file"', 'kind="image"'), after), true);
  assert.equal(modelImageEchoMatches(original[0], pair.join(" ")), true, "an image-only send has no caption");
});

test("model-copy matching requires every original path, caption, label and paired copy", () => {
  for (const changed of [
    after.replace("Inspect these:", "Different text:"),
    after.replace("document.pdf\"", "different.pdf\""),
    after.replace('label="image1"', 'label="image2"'),
    after.replace('label="original"', 'label="other"'),
    after.replace("opaque-model.jpg", "unrelated.jpg"),
    after.replace(pair[0], ""),
    `${after} ${marker("/extra/file.bin", "file")}`,
    [pair[0], pair[1], ...original.slice(1)].reverse().join(" "),
  ]) assert.equal(modelImageEchoMatches(before, changed), false, changed);
  assert.equal(modelImageEchoMatches(before, before), false, "ordinary echoes use their existing matcher");
});
