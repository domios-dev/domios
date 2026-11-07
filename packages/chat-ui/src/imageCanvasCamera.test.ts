import assert from "node:assert/strict";
import { test } from "node:test";
import { computeFitTransform, type ViewportTransform } from "@codeterm/canvas-camera";
import { createImageCanvasCamera, imageCanvasKey } from "./imageCanvasCamera";
import { galleryNext, layoutGallery } from "./galleryModel";

test("camera fits on first open, keeps user position on select and navigate, restores on reopen, and fits explicitly", () => {
  const memory = new Map<string, ViewportTransform>();
  const bounds = layoutGallery([{ width: 800, height: 600 }, { width: 400, height: 300 }]).bounds;
  const camera = createImageCanvasCamera("gallery", memory);
  assert.equal(camera.initialize(bounds, 0, 0), null);
  assert.equal(camera.initialize(bounds, 800, 600, false), null, "placeholder bounds cannot consume the initial fit");
  assert.deepEqual(camera.initialize(bounds, 800, 600), computeFitTransform(bounds, 800, 600));
  const moved = { x: -200, y: 70, scale: 2 };
  camera.remember(moved);
  for (const selected of [1, galleryNext(1, 1, 2)]) {
    assert.equal(camera.initialize({ ...bounds, x: selected * 840 }, 900, 700), null);
    assert.deepEqual(memory.get("gallery"), moved);
  }
  const reopened = createImageCanvasCamera("gallery", memory);
  assert.deepEqual(reopened.initialize(bounds, 1000, 600, false), moved, "saved cameras restore without waiting for measurements");
  assert.deepEqual(reopened.fit(bounds, 1000, 600), computeFitTransform(bounds, 1000, 600));
});

test("taking control while measurements are pending cancels the deferred fit and saves the camera", () => {
  const memory = new Map<string, ViewportTransform>();
  const camera = createImageCanvasCamera("pending", memory);
  const bounds = { x: 0, y: 0, width: 800, height: 600 };
  assert.equal(camera.initialize(bounds, 800, 600, false), null);
  const moved = { x: 100, y: -50, scale: 1.5 };
  camera.takeControl(moved);
  assert.equal(camera.initialize(bounds, 800, 600, true), null);
  assert.deepEqual(createImageCanvasCamera("pending", memory).initialize(bounds, 800, 600, false), moved);
});

test("camera memory and gallery keys stay bounded and separate galleries", () => {
  const memory = new Map<string, ViewportTransform>();
  const bounds = { x: 0, y: 0, width: 10, height: 10 };
  for (let index = 0; index < 100; index++) {
    const camera = createImageCanvasCamera(String(index), memory);
    camera.remember({ x: 0, y: 0, scale: 1 });
    assert(!memory.has(String(index)), "uninitialized paints are not saved");
    camera.initialize(bounds, 800, 600);
    camera.remember({ x: index, y: 0, scale: 1 });
    assert(memory.size <= 24);
  }
  assert(!memory.has("0"));
  assert(memory.has("99"));
  const source = "a".repeat(100_000);
  assert(imageCanvasKey([{ src: source }]).length < 40);
  assert.notEqual(imageCanvasKey([{ src: "a" }, { src: "b" }]), imageCanvasKey([{ src: "b" }, { src: "a" }]));
  assert.equal(imageCanvasKey([{ path: "/image.png", src: "" }]), imageCanvasKey([{ path: "/image.png", src: source }]));
});
