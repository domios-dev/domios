import assert from "node:assert/strict";
import { test } from "node:test";
import { computeFitTransform, flyTransform, touchTransform } from "@codeterm/canvas-camera";
import { galleryNext, layoutGallery, swipeDirection } from "./galleryModel";

test("gallery layout preserves image sizes, centres them and fits the entire set", () => {
  const layout = layoutGallery([{ width: 100, height: 200 }, { width: 300, height: 100 }]);
  assert.deepEqual(layout.items, [{ x: 0, y: 0, width: 100, height: 200 }, { x: 140, y: 50, width: 300, height: 100 }]);
  assert.deepEqual(layout.bounds, { x: 0, y: 0, width: 440, height: 200 });
  const fit = computeFitTransform(layout.bounds, 496, 256)!;
  assert.deepEqual(fit, { x: 28, y: 28, scale: 1 });
});

test("fit allows very large diagrams to open entirely inside a small phone viewport", () => {
  const fit = computeFitTransform({ x: 0, y: 0, width: 20000, height: 20000 }, 320, 480, { minScale: 0.0001 })!;
  assert.equal(fit.scale, 264 / 20000);
  assert.equal(fit.x, 28);
});

test("touch pinch holds the world point under a translating midpoint and clamps zoom", () => {
  const base = { x: 10, y: 20, scale: 1 };
  const before = [{ x: 50, y: 100 }, { x: 150, y: 100 }];
  const after = [{ x: 20, y: 130 }, { x: 220, y: 130 }];
  assert.deepEqual(touchTransform(base, before, after, 0.1, 8), { x: -60, y: -30, scale: 2 });
  assert.equal(touchTransform(base, before, after, 0.1, 1.5).scale, 1.5);
  assert.deepEqual(touchTransform(base, [{ x: 10, y: 10 }], [{ x: 40, y: 60 }], 0.1, 8), { x: 40, y: 70, scale: 1 });
  assert.deepEqual(touchTransform(base, [], [], 0.1, 8), base);
});

test("navigation wraps and only deliberate single finger horizontal swipes navigate", () => {
  assert.equal(galleryNext(2, 1, 3), 0);
  assert.equal(galleryNext(0, -1, 3), 2);
  assert.equal(galleryNext(0, 1, 0), 0);
  assert.equal(swipeDirection(-100, 10, 200, false), 1);
  assert.equal(swipeDirection(100, 10, 200, false), -1);
  assert.equal(swipeDirection(100, 10, 200, true), 0);
  assert.equal(swipeDirection(100, 80, 200, false), 0);
  assert.equal(swipeDirection(100, 10, 600, false), 0);
});

test("fly interpolation reaches exact endpoints and follows the cubic easing curve", () => {
  const from = { x: 0, y: 0, scale: 1 };
  const to = { x: 80, y: -40, scale: 3 };
  assert.deepEqual(flyTransform(from, to, 0), from);
  assert.deepEqual(flyTransform(from, to, 0.5), { x: 70, y: -35, scale: 2.75 });
  assert.deepEqual(flyTransform(from, to, 1), to);
  assert.deepEqual(flyTransform(from, to, 2), to);
});
