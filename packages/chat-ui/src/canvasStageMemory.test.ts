import assert from "node:assert/strict";
import { test } from "node:test";
import { createCanvasStageMemory, type CanvasStageMemory } from "./canvasStageMemory";

test("stage memory shares external and keyed camera state, clones paints and bounds galleries", () => {
  const store = new Map<string, CanvasStageMemory>();
  const external: CanvasStageMemory = { transform: null, userMoved: false };
  const camera = createCanvasStageMemory(undefined, external);
  const transform = { x: -200, y: 70, scale: 2 };
  camera.remember(transform, true);
  transform.x = 999;
  assert.deepEqual(external, { transform: { x: -200, y: 70, scale: 2 }, userMoved: true });
  for (let index = 0; index < 100; index++) {
    const memory = createCanvasStageMemory(String(index), undefined, store);
    assert.equal(store.has(String(index)), false, "uninitialized cameras are not saved");
    memory.remember({ x: index, y: 0, scale: 1 }, true);
    assert(store.size <= 24);
  }
  assert.equal(createCanvasStageMemory("0", undefined, store).read()!.transform, null);
  assert.deepEqual(createCanvasStageMemory("99", undefined, store).read(), { transform: { x: 99, y: 0, scale: 1 }, userMoved: true });
  createCanvasStageMemory("76", undefined, store).remember({ x: 1, y: 2, scale: 3 }, false);
  createCanvasStageMemory("next", undefined, store).remember(transform, true);
  assert(store.has("76"), "recently painted cameras survive eviction");
  assert(!store.has("77"));
});
