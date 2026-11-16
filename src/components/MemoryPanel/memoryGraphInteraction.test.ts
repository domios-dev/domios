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
const diameter = Math.hypot(0.1, 0.4) * 1.15;
assert.ok(Math.abs(wide.ratio - diameter) < 1e-10);
assert.equal(wide.angle, 0);
const tall = fitFramedNodes(points, { x: 0, y: 1.5 }, { x: 0.2, y: -0.5 });
assert.ok(tall);
assert.ok(Math.abs(tall.ratio - diameter / 0.2) < 1e-10);
assert.equal(fitFramedNodes([{ x: 0.35, y: 0.4 }], { x: 0, y: 0 }, { x: 1, y: 1 })?.ratio, 0.05);
assert.equal(fitFramedNodes([], { x: 0, y: 0 }, { x: 1, y: 1 }), null);
assert.equal(fitFramedNodes([{ x: NaN, y: Infinity }], { x: 0, y: 0 }, { x: 1, y: 1 }), null);
assert.equal(fitFramedNodes(points, { x: 0, y: 0 }, { x: 0, y: 1 }), null);
const cloud = Array.from({ length: 200 }, (_, i) => ({ x: i / 200, y: i / 400 }));
const viewportStart = { x: 0, y: 0 }, viewportEnd = { x: 1, y: 1 };
const baseline = fitFramedNodes(cloud, viewportStart, viewportEnd)!;
for (const outlier of [{ x: 1e9, y: -1e9 }, { x: -1e9, y: 1e9 }]) {
  const contaminated = [...cloud, outlier];
  const frame = fitFramedNodes(contaminated, viewportStart, viewportEnd)!;
  assert.ok(Math.abs(frame.ratio / baseline.ratio - 1) < 0.02);
  assert.ok(Math.abs(frame.x - baseline.x) < 0.01);
  assert.ok(Math.abs(frame.y - baseline.y) < 0.01);
  assert.deepEqual(fitFramedNodes(contaminated.reverse(), viewportStart, viewportEnd), frame);
}
assert.deepEqual(fitFramedNodes([...cloud, { x: NaN, y: Infinity }], viewportStart, viewportEnd), baseline);

const roundCloud = Array.from({ length: 200 }, (_, i) => {
  const angle = i * Math.PI * 2 / 200;
  return { x: Math.cos(angle), y: Math.sin(angle) };
});
const halo = [{ x: 1.14, y: 0 }, { x: -1.14, y: 0 }, { x: 0, y: 1.14 }, { x: 0, y: -1.14 }];
const roundFrame = fitFramedNodes([...roundCloud, ...halo], viewportStart, viewportEnd)!;
assert.ok(Math.abs(roundFrame.x) < 1e-10 && Math.abs(roundFrame.y) < 1e-10);
assert.ok(Math.abs(roundFrame.ratio - 2.3) < 1e-10);
for (const point of halo) {
  assert.ok(Math.abs(point.x - roundFrame.x) < roundFrame.ratio / 2);
  assert.ok(Math.abs(point.y - roundFrame.y) < roundFrame.ratio / 2);
}
const rotated = [...roundCloud, ...halo].map(point => ({
  x: (point.x - point.y) / Math.SQRT2,
  y: (point.x + point.y) / Math.SQRT2,
}));
assert.ok(Math.abs(fitFramedNodes(rotated, viewportStart, viewportEnd)!.ratio - roundFrame.ratio) < 1e-10);
console.log("drag-threshold, framed-fit, and outlier-resilience scenarios passed");
