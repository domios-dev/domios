import assert from "node:assert/strict";
import test from "node:test";
import { parseAttachmentMarkers } from "@codeterm/chat-core";
import { capsuleSpans, capsuleToken, EMPTY_CAPSULES, serializeCapsuleDraft, type CapsuleRegistry } from "./composerCapsules";
import { createComposerUploads, draftHasPendingUploads, type ComposerUpload } from "./composerUploadModel";

function fixture(store: (blob: Blob, name: string) => Promise<string>) {
  let registry: CapsuleRegistry = EMPTY_CAPSULES;
  let draft = "left right";
  let caret = 5;
  let uploads: ComposerUpload[] = [];
  const controller = createComposerUploads({
    store, registry: () => registry, commit: (next) => { registry = next; },
    insert: (capsule) => {
      const inserted = `${capsuleToken(capsule.label)} `;
      draft = draft.slice(0, caret) + inserted + draft.slice(caret);
      caret += inserted.length;
    },
    publish: (next) => { uploads = next; },
  });
  return { controller, registry: () => registry, draft: () => draft, edit: (text: string) => { draft = text; }, uploads: () => uploads };
}

test("one pasted image reserves one inline chip and resolves one message reference at that position", async () => {
  let resolve!: (path: string) => void;
  let started!: () => void;
  const storing = new Promise<void>((done) => { started = done; });
  const h = fixture(() => new Promise<string>((done) => { resolve = done; started(); }));
  const result = h.controller.add(new Blob(["pixels"], { type: "image/png" }), "opaque.png");
  const initial = h.draft();
  assert.equal(capsuleSpans(initial).length, 1);
  assert.equal(draftHasPendingUploads(initial, h.registry()), true);
  h.edit(`${initial} more text`);
  await storing;
  resolve("/uploads/opaque.png");
  await result;
  assert.equal(h.draft(), `${initial} more text`);
  assert.equal(draftHasPendingUploads(h.draft(), h.registry()), false);
  const sent = serializeCapsuleDraft(h.draft(), h.registry());
  assert.deepEqual(parseAttachmentMarkers(sent).attachments.map((item) => item.path), ["/uploads/opaque.png"]);
  assert.equal(sent.split("/uploads/opaque.png").length - 1, 1);
  assert(sent.indexOf("left") < sent.indexOf("/uploads/opaque.png"));
  assert(sent.indexOf("/uploads/opaque.png") < sent.indexOf("right"));
  assert.equal(h.uploads().length, 0);
});

test("equal filenames from separate paste actions retain separate chips and complete in their original order", async () => {
  const resolvers: ((path: string) => void)[] = [];
  let started!: () => void;
  const storing = new Promise<void>((done) => { started = done; });
  const h = fixture(() => new Promise<string>((done) => { resolvers.push(done); if (resolvers.length === 2) started(); }));
  const first = h.controller.add(new Blob(["first"], { type: "image/png" }), "same.png");
  const second = h.controller.add(new Blob(["second"], { type: "image/png" }), "same.png");
  const before = capsuleSpans(h.draft()).map(({ label }) => label);
  assert.equal(new Set(before).size, 2);
  await storing;
  resolvers[1]("/uploads/second.png"); await second;
  resolvers[0]("/uploads/first.png"); await first;
  assert.deepEqual(parseAttachmentMarkers(serializeCapsuleDraft(h.draft(), h.registry())).attachments.map(({ path }) => path), ["/uploads/first.png", "/uploads/second.png"]);
});

test("retry resolves the reserved chip once and deleting a pending chip prevents its reappearance", async () => {
  let attempts = 0;
  const h = fixture(async () => { if (++attempts === 1) throw new TypeError("interrupted"); return "/uploads/opaque.png"; });
  await h.controller.add(new Blob(["pixels"], { type: "image/png" }), "opaque.png");
  const draft = h.draft();
  assert.equal(draftHasPendingUploads(draft, h.registry()), true);
  await h.controller.retry(h.uploads()[0].id);
  assert.equal(h.draft(), draft);
  assert.equal(parseAttachmentMarkers(serializeCapsuleDraft(draft, h.registry())).attachments.length, 1);
  const removed = fixture(async () => "/uploads/removed.png");
  const result = removed.controller.add(new Blob(["pixels"], { type: "image/png" }), "removed.png");
  removed.edit("left right");
  await result;
  assert.equal(removed.draft(), "left right");
  assert.equal(draftHasPendingUploads(removed.draft(), removed.registry()), false);
  assert.equal(parseAttachmentMarkers(serializeCapsuleDraft(removed.draft(), removed.registry())).attachments.length, 0);
});
