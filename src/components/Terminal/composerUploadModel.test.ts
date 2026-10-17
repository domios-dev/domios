import assert from "node:assert/strict";
import test from "node:test";
import { imageMimeType, parseAttachmentMarkers } from "@codeterm/chat-core";
import { runtimePlan } from "../../../plugins/provider-codex/src/runtime";
import type { RuntimeContext } from "@codeterm/plugin-sdk";
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

test("a video upload is labelled video1 and stored; an unknown container fails on the chip without storing", async () => {
  const stored: string[] = [];
  const mp4 = new Blob([new Uint8Array([0, 0, 0, 0x18, ...Buffer.from("ftypisom")])], { type: "video/mp4" });
  const h = fixture(async (_blob, name) => { stored.push(name); return "/uploads/clip.mp4"; });
  await h.controller.add(mp4, "ScreenRecording_10-14_1.mp4");
  assert.deepEqual(parseAttachmentMarkers(serializeCapsuleDraft(h.draft(), h.registry())).attachments.map(({ kind, label }) => [kind, label]), [["video", "video1"]]);
  const bad = fixture(async (_blob, name) => { stored.push(name); return "/uploads/bad.mp4"; });
  await bad.controller.add(new Blob([new Uint8Array(32)], { type: "video/mp4" }), "bad.mp4");
  assert.equal(bad.uploads()[0].state, "failed");
  assert.equal(bad.uploads()[0].failure?.retryable, false);
  assert.deepEqual(stored, ["ScreenRecording_10-14_1.mp4"]);
});

test("extensionless iOS JPEG retains mixed text and reaches the image attachment contract once", async () => {
  const h = fixture(async (_blob, name) => `/uploads/${name}`);
  await h.controller.add(new Blob(["pixels"], { type: "image/jpeg" }), "photo");
  const markers = parseAttachmentMarkers(serializeCapsuleDraft(h.draft(), h.registry()));
  assert.deepEqual(markers.attachments.map(({ kind, path }) => ({ kind, path })), [{ kind: "image", path: "/uploads/photo.jpg" }]);
  assert(markers.text.includes("left"));
  assert(markers.text.includes("right"));
  assert.equal(capsuleSpans(h.draft()).length, 1);
  const context: RuntimeContext = { epoch: 1, tabId: "opaque-tab", bindingNonce: "opaque-nonce", threadId: "opaque-thread", cwd: "/lane", hostVersion: "1.14.9", settings: {}, activeTurns: [], threadParents: [], claimedSends: [], providerSettings: {} };
  const plan = runtimePlan(context, { kind: "submitTurn", threadId: "opaque-thread", sendId: "opaque-send", settings: {}, input: {
    text: markers.text, attachments: markers.attachments.map(({ path }) => ({ id: `attachment:${path}`, path, mimeType: imageMimeType(path) })),
  } });
  assert.equal(plan.action.kind, "request");
  if (plan.action.kind !== "request") throw new Error("Expected image turn request");
  assert.equal(plan.action.call.method, "turn/start");
  assert.deepEqual((plan.action.call.params as { input: unknown[] }).input, [{ type: "text", text: markers.text }, { type: "localImage", path: "/uploads/photo.jpg" }]);
});

test("unsupported HEIC retains the draft and a failed chip without storing or permitting send", async () => {
  let stores = 0;
  const h = fixture(async () => { stores++; return "/uploads/photo.heic"; });
  await h.controller.add(new Blob(["pixels"], { type: "image/heic" }), "photo.heic");
  assert.equal(stores, 0);
  assert.equal(h.uploads()[0].state, "failed");
  assert.match(h.uploads()[0].failure!.message, /Unsupported image format/);
  assert.equal(draftHasPendingUploads(h.draft(), h.registry()), true);
  assert.equal(capsuleSpans(h.draft()).length, 1);
  assert(h.draft().includes("left"));
});
