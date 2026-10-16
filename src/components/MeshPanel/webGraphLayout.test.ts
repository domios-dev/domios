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
      const tightened = rectangles.map((rectangle, index) => {
        const centerX = rectangle.x + rectangle.width / 2;
        const centerY = rectangle.y + rectangle.height / 2;
        const factor = index === 0 ? 1 : 0.999;
        return {
          x: layout.web.x + (centerX - layout.web.x) * factor - rectangle.width / 2 - 10,
          y: layout.web.y + (centerY - layout.web.y) * factor - rectangle.height / 2 - 10,
          width: rectangle.width + 20, height: rectangle.height + 20,
        };
      });
      assert.ok(tightened.some((rectangle, index) => tightened.slice(index + 1).some((other) => intersects(rectangle, other))), "A smaller radius violates the reserved 20 px gap");
      assert.ok(layout.qrSize >= 280);
      assert.ok(layout.web.width < width);
      assert.equal(layout.web.x, width / 2);
      assert.equal(layout.web.y, 400);
      for (const [index, rectangle] of rectangles.entries()) {
        for (const other of rectangles.slice(index + 1)) assert.equal(intersects(rectangle, other), false, `${width}/${count}: Web and circular machine cards never overlap`);
      }
    }
  }
}
console.log("✓ 36 circular layouts: 1/2/8/15 machines at phone/desktop widths, varying Web heights, no overlap; readable QR fits viewport at default zoom");

const viewFor = (width: number, height: number, count: number) => {
  const layout = webGraphLayout(width, height, count);
  const nodes = [
    { x: layout.web.x - layout.web.width / 2, y: layout.web.y - layout.web.height / 2, width: layout.web.width, height: layout.web.height },
    ...layout.machines.map((machine) => ({ x: machine.x - MACHINE_CARD_WIDTH / 2, y: machine.y - MACHINE_CARD_HEIGHT / 2, width: MACHINE_CARD_WIDTH, height: MACHINE_CARD_HEIGHT })),
  ];
  const x = Math.min(...nodes.map((node) => node.x));
  const y = Math.min(...nodes.map((node) => node.y));
  const bounds = { x, y, width: Math.max(...nodes.map((node) => node.x + node.width)) - x, height: Math.max(...nodes.map((node) => node.y + node.height)) - y };
  const view = webGraphInitialView(width, height, bounds, layout.web, layout.qrSize);
  assert.ok(view.scale * layout.qrSize >= 200);
  assert.ok(Math.abs(view.bounds.width / 2 * view.scale - width / 2) < 1e-8);
  assert.ok(Math.abs(view.bounds.height / 2 * view.scale - height / 2) < 1e-8);
  return view;
};
assert.equal(viewFor(1440, 900, 8).mode, "fit");
assert.equal(viewFor(1440, 900, 15).mode, "web");
assert.equal(viewFor(390, 844, 8).mode, "web");
assert.equal(viewFor(390, 844, 15).mode, "web");
assert.equal(viewFor(1920, 1600, 15).mode, "fit");
console.log("✓ Initial camera fits readable inventories, otherwise centres Web at readable scale; both axes centre in the actual surface");
