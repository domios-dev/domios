import assert from "node:assert/strict";
import { test } from "node:test";
import { galleryPathPlan, loadGallerySlots } from "./galleryLoading";

test("gallery plans cap local reads at 32 while preserving order and repeated slots", () => {
  const plan = galleryPathPlan(Array.from({ length: 100 }, (_, index) => `/image-${index % 3}.png`));
  assert.equal(plan.paths.length, 32);
  assert.equal(plan.omitted, 68);
  assert.deepEqual(plan.paths.slice(0, 4), ["/image-0.png", "/image-1.png", "/image-2.png", "/image-0.png"]);
});

test("bounded workers deliver each slot progressively and continue after a refusal", async () => {
  const pending = new Map<string, { resolve: (value: string) => void; reject: (error: Error) => void }>();
  const started: string[] = [];
  const results: [number, { file: string } | { error: string }][] = [];
  const nextStart: (() => void)[] = [];
  const load = (path: string) => new Promise<string>((resolve, reject) => {
    started.push(path); pending.set(path, { resolve, reject }); nextStart.shift()?.();
  });
  const complete = loadGallerySlots(["a", "b", "c", "d"], load, (index, result) => results.push([index, result]), () => false);
  assert.deepEqual(started, ["a", "b"]);
  const thirdStarted = new Promise<void>(resolve => nextStart.push(resolve));
  pending.get("b")!.resolve("B"); await thirdStarted;
  assert.deepEqual(results, [[1, { file: "B" }]]);
  assert.deepEqual(started, ["a", "b", "c"]);
  const fourthStarted = new Promise<void>(resolve => nextStart.push(resolve));
  pending.get("a")!.reject(new Error("denied")); await fourthStarted;
  assert.deepEqual(results[1], [0, { error: "denied" }]);
  pending.get("c")!.resolve("C"); pending.get("d")!.resolve("D"); await complete;
  assert.equal(results.length, 4);
});

test("cancelled generations neither publish slots nor start further reads", async () => {
  let cancel = false;
  const finish: (() => void)[] = [];
  let starts = 0;
  const results: number[] = [];
  const complete = loadGallerySlots(["a", "b", "c"], () => {
    starts++;
    return new Promise<void>(resolve => finish.push(resolve));
  }, index => results.push(index), () => cancel);
  cancel = true; finish.forEach(resolve => resolve()); await complete;
  assert.equal(starts, 2);
  assert.deepEqual(results, []);
});

test("the loader itself enforces the gallery cap before issuing reads", async () => {
  const calls: string[] = [];
  const slots: number[] = [];
  await loadGallerySlots(Array.from({ length: 100 }, (_, index) => String(index)), async path => { calls.push(path); return path; }, index => slots.push(index), () => false);
  assert.equal(calls.length, 32);
  assert.equal(slots.length, 32);
});
