import assert from "node:assert/strict";
import test from "node:test";
import { extractTransferFiles, imageUploadName, isFileTransfer, planFileDrop } from "./composerTransferModel";

test("iOS items-only JPEG drop extracts one file despite an empty files list", () => {
  const photo = new File(["pixels"], "photo", { type: "image/jpeg" });
  const data = { files: [], types: ["public.jpeg"], items: [{ kind: "file", type: "image/jpeg", getAsFile: () => photo }] };
  assert.equal(isFileTransfer(data), true);
  assert.deepEqual(extractTransferFiles(data), [photo]);
  assert.equal(imageUploadName(photo.name, photo.type), "photo.jpg");
});

test("desktop files and items representations produce one chip input", () => {
  const photo = new File(["pixels"], "photo.png", { type: "image/png", lastModified: 1 });
  const duplicate = new File(["pixels"], "photo.png", { type: "image/png", lastModified: 1 });
  assert.deepEqual(extractTransferFiles({ files: [photo], items: [{ kind: "file", type: photo.type, getAsFile: () => duplicate }] }), [photo]);
});

test("clipboard paste whose items mint a fresh File per read yields one file", () => {
  const listed = new File(["pixels"], "image.png", { type: "image/png", lastModified: 1 });
  const minted = () => new File(["pixels"], "image.png", { type: "image/png", lastModified: Date.now() + 1000 });
  assert.deepEqual(extractTransferFiles({ files: [listed], items: [{ kind: "file", type: "image/png", getAsFile: minted }] }), [listed]);
});

test("HEIC and unreadable photo drags are recognized for cancellation and visible refusal", () => {
  for (const type of ["image/heic", "public.heic", "public.jpeg"]) {
    const data = { files: [], items: [{ kind: "file", type, getAsFile: () => null }], types: [type] };
    assert.equal(isFileTransfer(data), true);
    assert.deepEqual(extractTransferFiles(data), []);
    assert.equal(planFileDrop(data).preventNavigation, true);
    assert(planFileDrop(data).error);
  }
  assert.throws(() => imageUploadName("photo.heic", "image/heic"), /Unsupported image format/);
  assert.throws(() => imageUploadName("photo.heic", ""), /Unsupported image format/);
});

test("image URL drops refuse visibly without navigating; all composer drops cancel navigation", () => {
  assert(planFileDrop({ types: ["text/uri-list"] }).error);
  assert.equal(planFileDrop({ types: ["text/uri-list"] }).preventNavigation, true);
  assert.equal(planFileDrop({ types: [] }).preventNavigation, true);
});

test("mixed text and image keeps the file; plain text is not intercepted", () => {
  const photo = new File(["pixels"], "photo.jpg", { type: "image/jpeg" });
  assert.deepEqual(extractTransferFiles({ files: [photo], types: ["text/plain", "Files"] }), [photo]);
  assert.equal(isFileTransfer({ types: ["text/plain"] }), false);
});
