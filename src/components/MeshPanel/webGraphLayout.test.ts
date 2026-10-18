import assert from "node:assert/strict";
import { webGraphLayout, webGraphInitialView, MACHINE_CARD_WIDTH, MACHINE_CARD_HEIGHT } from "./webGraphLayout";

type Rectangle = { x: number; y: number; width: number; height: number };
const intersects = (a: Rectangle, b: Rectangle) => a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
for (const width of [390, 1280, 1920]) {
  for (const count of [1, 2, 8, 15]) {
    for (const webHeight of [640, 900, 1200]) {
      const layout = webGraphLayout(width, 800, count, webHeight);
      const rectangles = [
        { x: layout.web.x - layout.web.width / 2, y: layout.web.y - webHeight / 2, width: layout.web.width, height: webHeight },
        ...layout.machines.map((node) => ({ x: node.x - MACHINE_CARD_WIDTH / 2, y: node.y - MACHINE_CARD_HEIGHT / 2, width: MACHINE_CARD_WIDTH, height: MACHINE_CARD_HEIGHT })),
      ];
      assert.equal(layout.machines.length, count);
      assert.equal(layout.qrSize, 128);
      assert.ok(layout.web.width < width);
      assert.equal(layout.web.x, width / 2);
      assert.equal(layout.web.y, 400);
      for (const [index, rectangle] of rectangles.entries()) {
        for (const other of rectangles.slice(index + 1)) assert.equal(intersects(rectangle, other), false, `${width}/${count}: Web and circular machine cards never overlap`);
      }
    }
  }
}
console.log("✓ 36 circular layouts: 1/2/8/15 machines at phone/desktop widths, varying Web heights, no overlap; compact QR");

const viewFor = (width: number, height: number, count: number) => {
  const layout = webGraphLayout(width, height, count);
  const nodes = [
    { x: layout.web.x - layout.web.width / 2, y: layout.web.y - layout.web.height / 2, width: layout.web.width, height: layout.web.height },
    ...layout.machines.map((machine) => ({ x: machine.x - MACHINE_CARD_WIDTH / 2, y: machine.y - MACHINE_CARD_HEIGHT / 2, width: MACHINE_CARD_WIDTH, height: MACHINE_CARD_HEIGHT })),
  ];
  const x = Math.min(...nodes.map((node) => node.x));
  const y = Math.min(...nodes.map((node) => node.y));
  const bounds = { x, y, width: Math.max(...nodes.map((node) => node.x + node.width)) - x, height: Math.max(...nodes.map((node) => node.y + node.height)) - y };
  const view = webGraphInitialView(width, height, bounds)!;
  assert.ok(view.scale * bounds.width <= width + 1e-8);
  assert.ok(view.scale * bounds.height <= height + 1e-8);
  assert.deepEqual(view.bounds, bounds);
  return view;
};
for (const [width, height, count] of [[1440, 900, 2], [1440, 900, 15], [390, 844, 15]]) viewFor(width, height, count);
assert.equal(webGraphInitialView(800, 600, { x: 0, y: 0, width: 1000, height: 900 }, true), null);
assert.equal(webGraphInitialView(0, 600, { x: 0, y: 0, width: 1000, height: 900 }), null);
console.log("Initial view fits all nodes and preserves user adjustments");

const balanced = webGraphLayout(1600, 900, 4, 320, [160, 240, 180, 200]);
assert.equal(balanced.web.x, 800);
assert.equal(balanced.web.y, 450);
const gaps = balanced.machines.map((machine, index) => index % 2
  ? Math.abs(machine.x - balanced.web.x) - (balanced.web.width + MACHINE_CARD_WIDTH) / 2
  : Math.abs(machine.y - balanced.web.y) - (balanced.web.height + machine.height) / 2);
for (const gap of gaps) assert.ok(Math.abs(gap - balanced.gap) < 1e-8);
console.log("Measured cardinal cards have equal gaps from the exact center");
