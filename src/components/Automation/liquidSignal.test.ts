import assert from "node:assert/strict";
import { liquidSignal, liquidStretch } from "./liquidSignal";

assert.deepEqual(liquidStretch(0), { scaleX: 1, scaleY: 1 });
assert.equal(liquidStretch(1).scaleX, 1.12);
assert.equal(liquidStretch(-1).scaleX, 1.12);
assert.equal(liquidStretch(100).scaleX, 1.32);
assert.equal(liquidStretch(0, 1).scaleX, 1.22);
assert.equal(liquidStretch(0, -1).scaleX, 0.78);
for (const velocity of [-10, -1, 0, 1, 10]) {
  const { scaleX, scaleY } = liquidStretch(velocity);
  assert.ok(Math.abs(scaleX * scaleY * scaleY - 1) < 1e-12);
}
assert.equal(liquidSignal(0).distance, 0);
assert.equal(liquidSignal(0.4).distance, 0.5);
assert.equal(liquidSignal(0.8).distance, 1);
assert.equal(liquidSignal(1).distance, 1);
assert.ok(liquidSignal(0.08).distance < 0.1, "departure is eased");
assert.ok(liquidSignal(0.72).distance > 0.9, "arrival is eased");
assert.equal(liquidSignal(0.8).scaleX, 1.22);
assert.ok(liquidSignal(0.86).scaleX < 1, "landing squash reverses after impact");
assert.ok(Math.abs(liquidSignal(0.99).scaleX - 1) < 0.01, "the landing oscillation settles");
assert.equal(liquidSignal(0).opacity, 0);
assert.ok(Math.abs(liquidSignal(1).opacity) < 1e-9, "the droplet fades out at arrival");
console.log("Liquid signals ease along the native path tangent with velocity stretch and damped landing");
