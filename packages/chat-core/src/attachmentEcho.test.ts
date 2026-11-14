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

const unavailableNote = " [Model preview unavailable: This host cannot create a model preview for this image format; use the original file path or attach a JPEG or PNG. Original kept.]";

test("a host conversion-unavailable suffix preserves the original attachment and caption identity", () => {
  const file = marker("C:/chat/phone.heic", "file", "IMG_4321.heic");
  const sent = `Check this ${file} after`;
  const echo = `Check this ${file}${unavailableNote} after`;
  assert.equal(modelImageEchoMatches(sent, echo), true);
  assert.equal(modelImageEchoMatches(sent, `<pasted_content id="own">${echo}</pasted_content id="own">`), true);
  assert.equal(modelImageEchoMatches(sent.replace('kind="file"', 'kind="image"'), echo), true);
  assert.equal(modelImageEchoMatches(file, file + unavailableNote), true);
  assert.equal(modelImageEchoMatches(`${sent} ${before.replaceAll("phone.heic", "large.jpg")}`, `${echo} ${after.replaceAll("phone.heic", "large.jpg")}`), true);
  for (const changed of [echo.replace("Check this", "Changed caption"), echo.replace("Original kept.", "Not kept."),
    echo.replace("IMG_4321.heic", "other.heic"), echo.replace("phone.heic", "other.heic"), `${echo} extra`,
    `${sent}${unavailableNote}`, echo.replace("phone.heic", "document.pdf")]) {
    assert.equal(modelImageEchoMatches(sent, changed), false, changed);
  }
  const document = marker("C:/chat/document.pdf", "file", "document.pdf");
  assert.equal(modelImageEchoMatches(document, document + unavailableNote), false);
});
