import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { composerIssues, MAX_TEMP_FILE_DECODED_BYTES } from "./composerIssues";
import { createAttachmentUploads, classifyUploadFailure, type AttachmentUpload } from "./attachmentUploadModel";

const failed = (error: unknown, size = 10): AttachmentUpload => ({
  id: 1, name: "voice-note.webm", blob: { size, type: "audio/webm" } as Blob,
  state: "failed", failure: classifyUploadFailure(error),
});

test("the generated client limit matches the backend source", () => {
  const source = readFileSync(new URL("../../../src-tauri/crates/codeterm-api/src/web_routes.rs", import.meta.url), "utf8");
  const expression = source.match(/pub const MAX_TEMP_FILE_DECODED_BYTES: usize = ([\d *]+);/)?.[1];
  assert.ok(expression);
  assert.equal(MAX_TEMP_FILE_DECODED_BYTES, expression.split("*").reduce((value, factor) => value * Number(factor.trim()), 1));
});

test("oversized recording names its size and the shared limit, and only offers an existing path", () => {
  const upload = failed(new Error("size"), MAX_TEMP_FILE_DECODED_BYTES + 1);
  const [issue] = composerIssues([], [upload]);
  assert.equal(issue.kind, "too-large");
  assert.match(issue.title, /151 MB, limit 150 MB/);
  assert.deepEqual(issue.actions, ["remove"]);
  assert.match(issue.explanation!, /no saved file path/);
  assert.deepEqual(composerIssues([{ uploadId: 1, path: "/recordings/note.webm" }], [upload])[0].actions, ["send-path", "remove"]);
});

test("HTTP 413 is humanized and keeps the route in details", () => {
  const message = "/api/attachments/temp-file → HTTP 413: length limit exceeded";
  const [issue] = composerIssues([], [failed(Object.assign(new Error(message), { status: 413 }))]);
  assert.equal(issue.kind, "too-large");
  assert.ok(!issue.title.includes("/api/"));
  assert.ok(issue.detail.includes(message));
  assert.deepEqual(issue.actions, ["remove"]);
});

test("network and server failures expose retry and removal with human reasons", () => {
  for (const [error, reason] of [[new TypeError("Failed to fetch"), "connection interrupted"], [Object.assign(new Error("/api/upload HTTP 503"), { status: 503 }), "server unavailable"]] as const) {
    const [issue] = composerIssues([], [failed(error)]);
    assert.equal(issue.title, `Upload failed: ${reason}`);
    assert.deepEqual(issue.actions, ["retry", "remove"]);
  }
});

test("over-limit files are rejected before reading or sending; removal clears the issue", async () => {
  let uploads: AttachmentUpload[] = [];
  const controller = createAttachmentUploads({
    store: async () => assert.fail("must not send oversized bytes"), attach: () => assert.fail("must not attach"),
    publish: next => { uploads = next; },
  });
  await controller.add({ size: MAX_TEMP_FILE_DECODED_BYTES + 1, type: "", arrayBuffer: () => assert.fail("must not read oversized bytes") } as unknown as Blob, "large.zip");
  assert.equal(composerIssues([], uploads)[0].kind, "too-large");
  await controller.retry(uploads[0].id);
  controller.remove(uploads[0].id);
  assert.deepEqual(composerIssues([], uploads), []);
});

test("a successful retry clears issues and preserves the captured attachment", async () => {
  let uploads: AttachmentUpload[] = [];
  let attempts = 0;
  const paths: string[][] = [];
  const controller = createAttachmentUploads({
    store: async () => { if (++attempts === 1) throw new TypeError("network"); return "/saved/file.txt"; },
    attach: next => { paths.push(next); }, publish: next => { uploads = next; },
  });
  await controller.add(new Blob(["file"]), "file.txt");
  assert.equal(composerIssues([], uploads).length, 1);
  await controller.retry(uploads[0].id);
  assert.deepEqual(composerIssues([], uploads), []);
  assert.deepEqual(paths, [["/saved/file.txt"]]);
});
