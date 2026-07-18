import assert from "node:assert/strict";
import { clampFloatingPosition } from "./floatingPosition";

const topLeft = clampFloatingPosition(-20, -10, 180, 120, 800, 600);
assert.deepEqual(topLeft, { top: 4, left: 4, maxHeight: 592 });

const topRight = clampFloatingPosition(790, -10, 180, 120, 800, 600);
assert.equal(topRight.left, 616);
assert.equal(topRight.top, 4);

const bottomLeft = clampFloatingPosition(-20, 590, 180, 120, 800, 600);
assert.equal(bottomLeft.left, 4);
assert.equal(bottomLeft.top, 476);

const bottomRight = clampFloatingPosition(790, 590, 180, 120, 800, 600);
assert.deepEqual(bottomRight, { top: 476, left: 616, maxHeight: 120 });

console.log("floating position tests: passed");
