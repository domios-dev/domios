import { formatAttachmentMarker, parseAttachmentMarkers } from "@codeterm/chat-core";
import assert from "node:assert/strict";
import {
  absorbAttachmentTags,
  addCapsule,
  capsuleArrowTarget,
  capsuleDeletionRange,
  capsuleSegments,
  capsuleSpans,
  capsuleToken,
  capsulesForPaths,
  capsulesInDraft,
  decodeCapsuleRegistry,
  EMPTY_CAPSULES,
  encodeCapsuleRegistry,
  expandToCapsules,
  hasCapsuleTokens,
  isLongPaste,
  serializeCapsuleDraft,
} from "./composerCapsules";

const SHOT_1 = String.raw`C:\Users\User\AppData\Local\Temp\codeterm-chat\a1-pasted-image-1.png`;
const SHOT_2 = String.raw`C:\Users\User\AppData\Local\Temp\codeterm-chat\b2-pasted-image-2.png`;
const NOTES = String.raw`C:\Users\User\AppData\Local\Temp\codeterm-chat\c3-pasted-text-3.txt`;

// numbering per draft, same path keeps its label, files and folders use their names
{
  let registry = EMPTY_CAPSULES;
  const first = addCapsule(registry, { kind: "image", path: SHOT_1 });
  registry = first.registry;
  const second = addCapsule(registry, { kind: "image", path: SHOT_2 });
  registry = second.registry;
  assert.equal(first.capsule.label, "image1");
  assert.equal(second.capsule.label, "image2");
  assert.equal(addCapsule(registry, { kind: "image", path: SHOT_1.replace(/\\/g, "/") }).capsule.label, "image1");
  const text = addCapsule(registry, { kind: "text", path: NOTES, lines: 312 });
  assert.equal(text.capsule.label, "text1 · 312 lines");
  assert.equal(addCapsule(EMPTY_CAPSULES, { kind: "text", path: NOTES, lines: 1 }).capsule.label, "text1 · 1 line");
  const file = addCapsule(text.registry, { kind: "file", path: "/home/me/report.pdf" });
  assert.equal(file.capsule.label, "report.pdf");
  assert.equal(addCapsule(file.registry, { kind: "file", path: "/other/report.pdf" }).capsule.label, "report.pdf (2)");
  assert.equal(addCapsule(EMPTY_CAPSULES, { kind: "folder", path: "C:\\work\\repo\\" }).capsule.label, "repo");
  const long = addCapsule(EMPTY_CAPSULES, { kind: "file", path: "/x/a-very-long-file-name-that-keeps-going-forever.tar.gz" });
  assert.ok(long.capsule.label.length <= 32 && long.capsule.label.endsWith(".tar.gz"), "a long name keeps its extension");
}

// numbering keeps counting after a capsule is deleted, so undo never collides
{
  const one = addCapsule(EMPTY_CAPSULES, { kind: "image", path: SHOT_1 });
  assert.equal(addCapsule(one.registry, { kind: "image", path: SHOT_2 }).capsule.label, "image2");
}

// serialize/parse round trip: the agent sees tags with absolute paths, absorbing them restores the draft
{
  const { registry: withImage, text: imageToken } = capsulesForPaths(EMPTY_CAPSULES, [{ path: SHOT_1 }]);
  const { registry, capsule } = addCapsule(withImage, { kind: "text", path: NOTES, lines: 312 });
  const draft = `look ${imageToken} here\nand ${capsuleToken(capsule.label)} done`;
  const sent = serializeCapsuleDraft(draft, registry);
  assert.equal(
    sent,
    `look ${formatAttachmentMarker(SHOT_1, "image", "image1")} here\nand ${formatAttachmentMarker(NOTES, "text", "text1 · 312 lines")} done`,
  );
  assert.ok(sent.includes(SHOT_1) && sent.includes(NOTES), "absolute paths stay readable");
  assert.deepEqual(parseAttachmentMarkers(sent).attachments.map((a) => a.path), [SHOT_1, NOTES]);
  const restored = absorbAttachmentTags(sent, EMPTY_CAPSULES);
  assert.equal(restored.text, draft);
  assert.deepEqual(restored.registry, registry);
  assert.deepEqual(capsulesInDraft(draft, registry).map((c) => c.label), ["image1", "text1 · 312 lines"]);
}

// absorbing a tag whose label is taken by another path renumbers it
{
  const { registry } = addCapsule(EMPTY_CAPSULES, { kind: "image", path: SHOT_1 });
  const absorbed = absorbAttachmentTags(`x ${formatAttachmentMarker(SHOT_2, "image", "image1")}`, registry);
  assert.equal(absorbed.text, `x ${capsuleToken("image2")}`);
  assert.equal(absorbed.registry.image2?.path, SHOT_2);
}

// tokens are single atoms with known spans; plain text is untouched
{
  const token = capsuleToken("text1 · 3 lines");
  assert.ok(!/[ \n]/.test(token), "a token never wraps or breaks lines");
  const text = `a ${token} b`;
  assert.deepEqual(capsuleSpans(text), [{ start: 2, end: 2 + token.length, label: "text1 · 3 lines" }]);
  assert.equal(hasCapsuleTokens("plain old draft"), false);
  assert.equal(hasCapsuleTokens(text), true);
  assert.equal(serializeCapsuleDraft("plain old draft", EMPTY_CAPSULES), "plain old draft");
  assert.equal(serializeCapsuleDraft(text, EMPTY_CAPSULES), "a text1 · 3 lines b", "an unknown token degrades to its label");
  const segments = capsuleSegments(text, EMPTY_CAPSULES);
  assert.equal(segments.length, 3);
  assert.deepEqual(segments[0], { text: "a " });
  assert.ok("span" in segments[1] && segments[1].capsule === null && segments[1].token === token);
}

// Backspace and Delete remove a whole capsule; partial selections widen
{
  const token = capsuleToken("image1");
  const text = `ab${token}cd`;
  const start = 2;
  const end = 2 + token.length;
  assert.deepEqual(capsuleDeletionRange(text, end, end, "Backspace"), { start, end });
  assert.deepEqual(capsuleDeletionRange(text, start, start, "Delete"), { start, end });
  assert.equal(capsuleDeletionRange(text, start, start, "Backspace"), null, "Backspace before the capsule deletes text");
  assert.equal(capsuleDeletionRange(text, end, end, "Delete"), null);
  assert.deepEqual(capsuleDeletionRange(text, 1, start + 3, "Backspace"), { start: 1, end });
  assert.equal(capsuleDeletionRange(text, 0, text.length, "Delete"), null, "a selection already covering capsules stays native");
  const removed = text.slice(0, start) + text.slice(end);
  assert.equal(removed, "abcd");
}

// arrows jump over a capsule; a caret inside one snaps to the nearest edge
{
  const token = capsuleToken("image1");
  const text = `ab${token}cd`;
  const end = 2 + token.length;
  assert.equal(capsuleArrowTarget(text, end, "left"), 2);
  assert.equal(capsuleArrowTarget(text, 2, "right"), end);
  assert.equal(capsuleArrowTarget(text, 2, "left"), null);
  assert.equal(capsuleArrowTarget(text, end, "right"), null);
  assert.deepEqual(expandToCapsules(text, 3, 3), { start: 2, end: 2 });
  assert.deepEqual(expandToCapsules(text, end - 1, end - 1), { start: end, end });
  assert.deepEqual(expandToCapsules(text, 1, 4), { start: 1, end });
}

// undo: deleting a token keeps the registry, so restoring the text restores the capsule
{
  const { registry, text: token } = capsulesForPaths(EMPTY_CAPSULES, [{ path: SHOT_1 }]);
  const before = `see ${token}`;
  const after = "see ";
  assert.deepEqual(capsulesInDraft(after, registry), []);
  assert.deepEqual(capsulesInDraft(before, registry).map((c) => c.path), [SHOT_1]);
}

// long paste threshold
assert.equal(isLongPaste("short"), false);
assert.equal(isLongPaste("x".repeat(2001)), true);
assert.equal(isLongPaste(Array.from({ length: 41 }, (_, i) => `line ${i}`).join("\n")), true);
assert.equal(isLongPaste(Array.from({ length: 40 }, (_, i) => `line ${i}`).join("\n")), false);

// draft persistence: registry encodes to a string and survives garbage
{
  const { registry } = capsulesForPaths(EMPTY_CAPSULES, [{ path: SHOT_1 }, { path: "/srv/repo", kind: "folder" }]);
  const raw = encodeCapsuleRegistry(registry);
  assert.deepEqual(decodeCapsuleRegistry(raw), registry);
  assert.equal(encodeCapsuleRegistry(EMPTY_CAPSULES), "", "an empty registry clears its key");
  assert.deepEqual(decodeCapsuleRegistry(""), {});
  assert.deepEqual(decodeCapsuleRegistry("{not json"), {});
  assert.deepEqual(decodeCapsuleRegistry('[{"label":"x","kind":"bogus","path":"/p"}]'), {});
}

// context capsules: quotes number per draft, given labels stay and repeat with a suffix, all serialize as one tag family
{
  const { registry, text } = capsulesForPaths(EMPTY_CAPSULES, [
    { path: "/t/q1.md", kind: "quote", detail: "domios-after.md:42" },
    { path: "/t/q2.md", kind: "quote", detail: "Fermi 14:05" },
    { path: "/t/m1.md", kind: "message", label: "msg · Fermi 14:05" },
    { path: "/t/m2.md", kind: "message", label: "msg · Fermi 14:05" },
    { path: "/t/d.diff", kind: "diff", label: "diff · ChangesPanel.tsx +26 −4" },
    { path: "/repo/notes.md", kind: "document" },
  ]);
  assert.deepEqual(capsuleSpans(text).map((span) => span.label), [
    "quote1 · domios-after.md:42", "quote2 · Fermi 14:05", "msg · Fermi 14:05", "msg · Fermi 14:05 (2)", "diff · ChangesPanel.tsx +26 −4", "notes.md",
  ]);
  const sent = serializeCapsuleDraft(text, registry);
  assert.deepEqual(parseAttachmentMarkers(sent).attachments.map(({ kind, label }) => `${kind}:${label}`), [
    "quote:quote1 · domios-after.md:42", "quote:quote2 · Fermi 14:05", "message:msg · Fermi 14:05", "message:msg · Fermi 14:05 (2)",
    "diff:diff · ChangesPanel.tsx +26 −4", "document:notes.md",
  ]);
  assert.deepEqual(decodeCapsuleRegistry(encodeCapsuleRegistry(registry)), registry, "new kinds survive draft persistence");
  const pasted = absorbAttachmentTags(formatAttachmentMarker("/t/q9.md", "quote", "quote1 · domios-after.md:42"), registry);
  assert.equal(capsuleSpans(pasted.text)[0].label, "quote3 · domios-after.md:42", "a pasted quote renumbers and keeps its detail");
}

console.log("composerCapsules tests passed");
