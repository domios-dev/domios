import assert from "node:assert/strict";
import test from "node:test";
import { extractTransferFiles, identifyOpaqueImage, imageUploadName, isFileTransfer, planFileDrop } from "./composerTransferModel";

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
  assert.equal(imageUploadName("photo.heic", "image/heic"), "photo.heic");
  assert.equal(imageUploadName("photo.heic", ""), "photo.heic");
});

test("an image type on the drop item names an opaque Safari file only once", () => {
  const bytes = new File(["pixels"], "image.bin");
  const item = { kind: "file", type: "public.heic", getAsFile: () => bytes };
  const result = planFileDrop({ files: [bytes], items: [item], types: ["Files"] });
  assert.deepEqual(result.files.map(file => file.name), ["image.heic"]);
  assert.equal(result.files[0].size, bytes.size);
  assert.equal(result.error, null);
  const unreadable = planFileDrop({ types: ["public.jpeg"], items: [
    { ...item, getAsFile: () => { throw new Error("external data withdrawn"); } },
  ] });
  assert.equal(unreadable.files.length, 0);
  assert.ok(unreadable.error);
});

test("a generic image UTI accepts known file bytes and refuses unreadable images", async () => {
  const data = { types: ["public.image"], files: [new File([new Uint8Array([0xff, 0xd8, 0xff])], "photo")] };
  const drop = planFileDrop(data);
  assert(drop.needsImageType);
  assert.equal((await identifyOpaqueImage(drop.files[0]))?.name, "photo.jpg");
  assert.equal(await identifyOpaqueImage(new File(["unreadable"], "photo")), null);
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
