import assert from "node:assert/strict";
import test from "node:test";
import { attachmentKindForUpload, MAX_VIDEO_BYTES, sniffVideoContainer, validateAttachment } from "./attachmentValidation";
import { attachmentKindForPath, formatAttachmentMarker, parseAttachmentMarkers } from "./attachmentMarker";
import { labelVideos, placedAttachmentSegments, userMessageAttachments } from "./userAttachments";

const ftyp = (brand: string) => new Uint8Array([0, 0, 0, 0x18, ...Buffer.from(`ftyp${brand}`)]);

test("upload kind follows the media type first and the extension second", () => {
  assert.equal(attachmentKindForUpload({ name: "opaque", mime: "video/quicktime" }), "video");
  assert.equal(attachmentKindForUpload({ name: "x.bin", mime: "image/png" }), "image");
  assert.equal(attachmentKindForUpload({ name: "ScreenRecording.MOV", mime: "" }), "video");
  assert.equal(attachmentKindForUpload({ name: "notes.txt", mime: "" }), "file");
  assert.equal(attachmentKindForPath("/u/a.webm"), "video");
});

test("containers are sniffed from leading bytes", () => {
  assert.equal(sniffVideoContainer(ftyp("isom")), "mp4");
  assert.equal(sniffVideoContainer(ftyp("qt  ")), "mov");
  assert.equal(sniffVideoContainer(new Uint8Array([0x1a, 0x45, 0xdf, 0xa3, 1, 2])), "webm");
  assert.equal(sniffVideoContainer(new Uint8Array([1, 2, 3])), null);
});

test("a video must be a known container within the size cap; other kinds pass", () => {
  assert.deepEqual(validateAttachment({ kind: "video", size: 10, head: ftyp("isom") }), { ok: true });
  const oversize = validateAttachment({ kind: "video", size: MAX_VIDEO_BYTES + 1, head: ftyp("isom") });
  assert.equal(oversize.ok, false);
  assert.equal(validateAttachment({ kind: "video", size: 10, head: new Uint8Array(16) }).ok, false);
  assert.deepEqual(validateAttachment({ kind: "image", size: MAX_VIDEO_BYTES * 2 }), { ok: true });
});

test("unlabeled video markers are numbered in order and the agent marker keeps the label", () => {
  const files = parseAttachmentMarkers([
    formatAttachmentMarker("/u/a.mp4"), formatAttachmentMarker("/u/b.mov"), formatAttachmentMarker("/u/c.txt"),
  ].join(" ")).attachments;
  assert.deepEqual(labelVideos(files).map((file) => file.label), ["video1", "video2", undefined]);
  assert.match(formatAttachmentMarker("/u/a.mp4", "video", "video1"), /kind="video".*label="video1"/);
  assert.deepEqual(userMessageAttachments(formatAttachmentMarker("/u/a.mp4")).files.map((file) => file.label), ["video1"]);
});

test("unlabeled inline videos read videoN without colliding with labelled ones", () => {
  const content = [
    formatAttachmentMarker("C:/a/one.mp4", "video"),
    formatAttachmentMarker("C:/a/two.mov", "video", "video1"),
    formatAttachmentMarker("C:/a/three.webm", "video"),
    formatAttachmentMarker("C:/a/shot.png", "image"),
  ].join(" ");
  const labels = placedAttachmentSegments(content).flatMap((segment) => "attachment" in segment ? [segment.attachment.label] : []);
  assert.deepEqual(labels, ["video2", "video1", "video3", undefined]);
  assert.deepEqual(labelVideos([{ kind: "video", path: "x.mp4" }, { kind: "video", path: "y.mp4", label: "video1" }]).map((file) => file.label), ["video2", "video1"]);
});
