import assert from "node:assert/strict";
import test from "node:test";
import { canRetryUpload, classifyUploadFailure, createAttachmentUploads, type AttachmentUpload } from "./attachmentUploadModel";

test("interrupted and malformed multipart uploads allow retry, permanent rejection does not", () => {
  for (const error of [new TypeError("network"), Object.assign(new Error("interrupted"), { name: "AbortError" }), Object.assign(new Error("Error parsing multipart/form-data request"), { status: 400 }), Object.assign(new Error("busy"), { status: 503 })]) {
    assert.equal(classifyUploadFailure(error).retryable, true);
  }
  for (const status of [400, 401, 403, 413]) assert.equal(classifyUploadFailure(Object.assign(new Error("rejected"), { status })).retryable, false);
});

test("retry preserves the captured bytes, suppresses duplicate retries, and attaches once", async () => {
  let uploads: AttachmentUpload[] = [];
  const paths: string[][] = [];
  let resolve!: (path: string) => void;
  let started!: () => void;
  const retryStarted = new Promise<void>((done) => { started = done; });
  let attempts = 0;
  const bytes: string[] = [];
  const controller = createAttachmentUploads({
    publish: (next) => { uploads = next; }, attach: (next) => paths.push(next),
    store: async (blob) => {
      bytes.push(await blob.text());
      if (++attempts === 1) throw new TypeError("network interrupted");
      return new Promise<string>((done) => { resolve = done; started(); });
    },
  });
  await controller.add(new Blob(["phone-image"], { type: "image/png" }), "photo.png");
  assert.equal(canRetryUpload(uploads[0]), true);
  const retry = controller.retry(uploads[0].id);
  assert.equal(canRetryUpload(uploads[0]), false);
  await controller.retry(uploads[0].id);
  await retryStarted;
  resolve("/uploads/photo.png");
  await retry;
  assert.deepEqual(bytes, ["phone-image", "phone-image"]);
  assert.deepEqual(paths, [["/uploads/photo.png"]]);
  assert.deepEqual(uploads, []);
});

test("removed or disposed uploads cannot attach after completion", async () => {
  for (const dispose of [false, true]) {
    let uploads: AttachmentUpload[] = [];
    let resolve!: (path: string) => void;
    let started!: () => void;
    const requestStarted = new Promise<void>((done) => { started = done; });
    const controller = createAttachmentUploads({
      publish: (next) => { uploads = next; }, attach: () => assert.fail("removed upload attached"),
      store: () => new Promise<string>((finish) => { resolve = finish; started(); }),
    });
    const upload = controller.add(new Blob(["x"]), "x.txt");
    await requestStarted;
    if (dispose) controller.dispose(); else controller.remove(uploads[0].id);
    resolve("/uploads/x.txt");
    await upload;
  }
});

test("voice upload returns its first path to the caller and a failed note retries once through the voice sink", async () => {
  const paths: string[][] = [];
  let uploads: AttachmentUpload[] = [];
  let fail = false;
  const controller = createAttachmentUploads({
    publish: (next) => { uploads = next; }, attach: (next) => paths.push(next),
    store: async () => { if (fail) throw new TypeError("interrupted"); return "/uploads/voice.webm"; },
  });
  assert.equal(await controller.add(new Blob(["voice"]), "voice.webm", false), "/uploads/voice.webm");
  assert.deepEqual(paths, []);
  fail = true;
  assert.equal(await controller.add(new Blob(["second"]), "second.webm", false), null);
  fail = false;
  await controller.retry(uploads[0].id);
  assert.deepEqual(paths, [["/uploads/voice.webm"]]);
});

test("a WebKit read failure retains the original file and retries reading before upload", async () => {
  let uploads: AttachmentUpload[] = [];
  const attached: string[][] = [];
  const file = new Blob(["image"], { type: "image/png" });
  const read = file.arrayBuffer.bind(file);
  let reads = 0;
  file.arrayBuffer = () => ++reads === 1
    ? Promise.reject(new DOMException("The I/O read operation failed.", "NotReadableError")) : read();
  const controller = createAttachmentUploads({
    publish: (next) => { uploads = next; }, attach: (paths) => attached.push(paths),
    store: async (blob) => { assert.equal(await blob.text(), "image"); return "/uploads/image.png"; },
  });
  await controller.add(file, "image.png");
  assert.equal(canRetryUpload(uploads[0]), true);
  assert.equal(uploads[0].blob, file);
  await controller.retry(uploads[0].id);
  assert.equal(reads, 2);
  assert.deepEqual(attached, [["/uploads/image.png"]]);
});

test("upload remains pending until its attachment is committed to the draft", async () => {
  let uploads: AttachmentUpload[] = [];
  let finishAttach!: () => void;
  let attachStarted!: () => void;
  const started = new Promise<void>(resolve => { attachStarted = resolve; });
  const controller = createAttachmentUploads({
    store: async () => "/uploads/file.txt",
    attach: () => new Promise<void>(resolve => { finishAttach = resolve; attachStarted(); }),
    publish: next => { uploads = next; },
  });
  const pending = controller.add(new Blob(["draft attachment"]), "file.txt");
  await started;
  assert.equal(uploads[0].state, "uploading");
  finishAttach();
  await pending;
  assert.deepEqual(uploads, []);
});
