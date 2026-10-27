import assert from "node:assert/strict";
import test from "node:test";
import { withinMessageHoverArea } from "./messageHoverGeometry";

const anchor = { left: 200, right: 220, top: 100, bottom: 120 };

for (const [top, left] of [[140, 100], [140, 220], [0, 100], [0, 220]]) {
  test(`keeps the pointer safe across the bridge to menu bounds at ${left}, ${top}`, () => {
    const menu = { left, right: left + 180, top, bottom: top + 80 };
    const nearY = top === 0 ? 80 : 140;
    const route = [
      { x: 210, y: 110 },
      { x: 210, y: top === 0 ? 99 : 121 },
      { x: (210 + left + 90) / 2, y: (110 + nearY) / 2 },
      { x: left + 90, y: top + 40 },
      { x: left + 185, y: top + 40 },
      { x: 210, y: 110 },
    ];
    assert.deepEqual(route.map(point => withinMessageHoverArea(point, anchor, menu)), [true, true, true, true, true, true]);
    assert.equal(withinMessageHoverArea({ x: left + 190, y: top + 40 }, anchor, menu), false);
  });
}

test("keeps a six-pixel margin at every menu edge and corner", () => {
  const menu = { left: 100, right: 280, top: 140, bottom: 220 };
  const points = [
    { x: 94, y: 180 }, { x: 286, y: 180 },
    { x: 140, y: 134 }, { x: 140, y: 226 },
    { x: 94, y: 134 }, { x: 286, y: 226 },
  ];
  assert.deepEqual(points.map(point => withinMessageHoverArea(point, anchor, menu)), [true, true, true, true, true, true]);
});

test("rejects points clearly beyond the margin or away from the bridge", () => {
  const menu = { left: 100, right: 280, top: 140, bottom: 220 };
  const points = [
    { x: 93, y: 180 }, { x: 287, y: 180 },
    { x: 94, y: 133 }, { x: 140, y: 227 },
    { x: 210, y: 90 }, { x: 100, y: 110 }, { x: 280, y: 110 },
  ];
  assert.deepEqual(points.map(point => withinMessageHoverArea(point, anchor, menu)), [false, false, false, false, false, false, false]);
});
