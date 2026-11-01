import assert from "node:assert/strict";
import test from "node:test";
import { inlineArtifactParts } from "./inlineArtifactParts";
import type { AttachmentSegment } from "./attachmentMarker";
const capsule: AttachmentSegment = { attachment: { kind: "file", path: "/work/note.txt", label: "note" } };
const tag = '<ds-image src="/work/a.png" />';
test("inlineArtifactParts preserves capsule order and canonical block spans", () => {
  assert.deepEqual(inlineArtifactParts([capsule, { text: `\n${tag}\n` }, capsule]), [capsule, { text: "\n" }, { directive: { kind: "image", path: "/work/a.png" } }, { text: "\n" }, capsule]);
  assert.deepEqual(inlineArtifactParts([{ text: '<ds-im' }, { text: 'age src="/work/a.png" />' }]), [{ directive: { kind: "image", path: "/work/a.png" } }]);
});
test("inlineArtifactParts never interprets inline or fenced examples", () => {
  for (const segments of [[{ text: `${tag} ` }, capsule], [capsule, { text: tag }], [{ text: "```\n" }, capsule, { text: `\n${tag}\n\`\`\`` }]] satisfies AttachmentSegment[][]) assert.ok(inlineArtifactParts(segments).every((part) => !("directive" in part)));
});

test("inlineArtifactParts keeps same-line capsules inert across text splits and CRLF boundaries", () => {
  for (let split = 0; split <= tag.length; split++) {
    const parts = inlineArtifactParts([{ text: tag.slice(0, split) }, { text: tag.slice(split) + " " }, capsule]);
    assert.ok(parts.every((part) => !("directive" in part)), `same-line capsule split ${split}`);
    assert.equal(parts.flatMap((part) => "text" in part ? [part.text] : []).join(""), tag + " ");
    assert.equal(parts[parts.length - 1], capsule);
  }
  assert.deepEqual(inlineArtifactParts([{ text: tag + "\r\n" }, capsule]), [{ directive: { kind: "image", path: "/work/a.png" } }, { text: "\r\n" }, capsule]);
});
