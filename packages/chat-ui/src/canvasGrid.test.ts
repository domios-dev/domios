import assert from "node:assert/strict";
import { test } from "node:test";
import { canvasGrid } from "./canvasGrid";

test("dots keep readable screen spacing and radius throughout the camera zoom range", () => {
  for (const scale of [0.0001, 0.001, 0.01, 0.1, 0.46, 0.82, 1, 2, 4, 8]) {
    const grid = canvasGrid({ x: 50, y: -20, scale });
    assert(grid.spacing >= 16 && grid.spacing < 32, `readable spacing at ${scale}`);
    assert(grid.radius >= 1 && grid.radius <= 1.5, `visible radius at ${scale}`);
    assert(Math.abs(grid.x + grid.spacing / 2 - 50) < 1e-9);
    assert(Math.abs(grid.y + grid.spacing / 2 + 20) < 1e-9);
  }
  assert.equal(canvasGrid({ x: 0, y: 0, scale: 0.46 }).spacing, 24 * 0.46 * 2);
});

test("adaptive dots remain attached to the camera origin and pan by the camera delta", () => {
  const before = canvasGrid({ x: -100, y: 70, scale: 0.46 });
  const after = canvasGrid({ x: -60, y: 45, scale: 0.46 });
  assert.equal(after.x - before.x, 40);
  assert.equal(after.y - before.y, -25);
  assert.equal(after.spacing, before.spacing);
});
