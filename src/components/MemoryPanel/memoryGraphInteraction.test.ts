import assert from "node:assert/strict";
import { fitFramedNodes, hasDragged } from "./memoryGraphInteraction";

assert.equal(hasDragged({ x: 10, y: 10 }, { x: 10, y: 10 }), false);
assert.equal(hasDragged({ x: 10, y: 10 }, { x: 12, y: 12 }), false);
assert.equal(hasDragged({ x: 10, y: 10 }, { x: 13, y: 14 }), true);
assert.equal(hasDragged({ x: 10, y: 10 }, { x: 4, y: 10 }), true);

const points = [{ x: 0.3, y: 0.2 }, { x: 0.4, y: 0.6 }];
const wide = fitFramedNodes(points, { x: -0.25, y: 1 }, { x: 1.25, y: 0 });
assert.ok(wide);
assert.equal(wide.x, 0.35);
assert.equal(wide.y, 0.4);
assert.ok(Math.abs(wide.ratio - 0.46) < 1e-10);
assert.equal(wide.angle, 0);
const tall = fitFramedNodes(points, { x: 0, y: 1.5 }, { x: 0.2, y: -0.5 });
assert.ok(tall);
assert.ok(Math.abs(tall.ratio - 0.575) < 1e-10);
assert.equal(fitFramedNodes([{ x: 0.35, y: 0.4 }], { x: 0, y: 0 }, { x: 1, y: 1 })?.ratio, 0.05);
assert.equal(fitFramedNodes([], { x: 0, y: 0 }, { x: 1, y: 1 }), null);
assert.equal(fitFramedNodes([{ x: NaN, y: Infinity }], { x: 0, y: 0 }, { x: 1, y: 1 }), null);
assert.equal(fitFramedNodes(points, { x: 0, y: 0 }, { x: 0, y: 1 }), null);
console.log("10 drag-threshold and framed-fit scenarios passed");
