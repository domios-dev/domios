import assert from "node:assert/strict";
import { anchoredPopoverPosition, pointAnchor, type FloatingPlacement, type FloatingViewport } from "./floatingPosition";

const phone = { width: 390, height: 700 };

const usagePill = anchoredPopoverPosition({ left: 350, right: 382, top: 240, bottom: 264 }, { width: 320, height: 300 }, phone, "bottom-end");
assert.deepEqual(usagePill, { top: 268, left: 62, maxHeight: 300, maxWidth: 374, side: "bottom", align: "end" }, "an end-aligned popover under a right-edge pill opens leftward, on screen");

const pillNearLeftEdge = anchoredPopoverPosition({ left: 20, right: 60, top: 240, bottom: 264 }, { width: 320, height: 300 }, phone, "bottom-end");
assert.equal(pillNearLeftEdge.left, 8, "an end-aligned popover that would cross the left edge is clamped to the margin");

const flipsAboveKeyboard = anchoredPopoverPosition({ left: 150, right: 182, top: 550, bottom: 574 }, { width: 240, height: 360 }, { width: 390, height: 600 }, "bottom-start");
assert.equal(flipsAboveKeyboard.side, "top", "a popover without room above the keyboard flips above its anchor");
assert.equal(flipsAboveKeyboard.top, 186);
assert.equal(flipsAboveKeyboard.maxHeight, 360);

const flipsBelowTitlebar = anchoredPopoverPosition({ left: 10, right: 40, top: 20, bottom: 44 }, { width: 240, height: 300 }, phone, "top-start");
assert.equal(flipsBelowTitlebar.side, "bottom", "a popover preferring above flips below when the screen top leaves no room");
assert.equal(flipsBelowTitlebar.top, 48);

const keepsPreferredWhenItFits = anchoredPopoverPosition({ left: 10, right: 40, top: 400, bottom: 424 }, { width: 240, height: 300 }, phone, "top-start");
assert.equal(keepsPreferredWhenItFits.side, "top");
assert.equal(keepsPreferredWhenItFits.top, 96);

const limitsToLargerSide = anchoredPopoverPosition({ left: 370, right: 402, top: 490, bottom: 514 }, { width: 340, height: 500 }, { width: 390, height: 540 }, "bottom-start");
assert.equal(limitsToLargerSide.side, "top", "with neither side fitting, the larger side wins");
assert.equal(limitsToLargerSide.maxHeight, 478, "the remaining room caps the height so the content scrolls");
assert.equal(limitsToLargerSide.top, 8);
assert.equal(limitsToLargerSide.left, 42);

const scrolledVisualViewport = anchoredPopoverPosition({ left: 100, right: 140, top: 600, bottom: 624 }, { width: 200, height: 260 }, { width: 390, height: 400, offsetTop: 300 }, "bottom-start");
assert.equal(scrolledVisualViewport.side, "top", "the visual viewport's offset and keyboard-shortened height bound the popover");
assert.ok(scrolledVisualViewport.top >= 308 && scrolledVisualViewport.top + scrolledVisualViewport.maxHeight <= 692);

const contextMenuInCorner = anchoredPopoverPosition(pointAnchor(380, 690), { width: 200, height: 150 }, phone, "bottom-start", { gap: 0 });
assert.deepEqual([contextMenuInCorner.left, contextMenuInCorner.top, contextMenuInCorner.side], [182, 540, "top"], "a context menu at the corner opens up and left of the point");

const tinyRoom = anchoredPopoverPosition({ left: 10, right: 40, top: 90, bottom: 110 }, { width: 100, height: 300 }, { width: 390, height: 200 }, "bottom-start");
assert.equal(tinyRoom.maxHeight, 96, "a cramped popover keeps a usable height rather than collapsing");
assert.ok(tinyRoom.top >= 8 && tinyRoom.top + tinyRoom.maxHeight <= 192);

const desktopBell = anchoredPopoverPosition({ left: 1480, right: 1500, top: 20, bottom: 44 }, { width: 360, height: 400 }, { width: 1600, height: 900 }, "bottom-end", { gap: 6 });
assert.deepEqual([desktopBell.left, desktopBell.top], [1140, 50], "a desktop anchor with room keeps its exact placement");

const centeredTip = anchoredPopoverPosition(pointAnchor(10, 300), { width: 240, height: 80 }, phone, "top-center");
assert.deepEqual([centeredTip.left, centeredTip.top, centeredTip.align], [8, 216, "center"], "a centered tip at the left edge clamps instead of hanging off");
const centeredMid = anchoredPopoverPosition({ left: 180, right: 210, top: 300, bottom: 320 }, { width: 100, height: 40 }, phone, "bottom-center");
assert.equal(centeredMid.left, 145);

const placements: FloatingPlacement[] = ["bottom-start", "bottom-center", "bottom-end", "top-start", "top-center", "top-end"];
const viewports: FloatingViewport[] = [
  { width: 390, height: 844 },
  { width: 390, height: 420 },
  { width: 390, height: 420, offsetTop: 200 },
  { width: 320, height: 568 },
  { width: 844, height: 390 },
];
let checked = 0;
for (const viewport of viewports) {
  const left = (viewport.offsetLeft ?? 0) + 8;
  const top = (viewport.offsetTop ?? 0) + 8;
  const right = (viewport.offsetLeft ?? 0) + viewport.width - 8;
  const bottom = (viewport.offsetTop ?? 0) + viewport.height - 8;
  for (let x = -40; x <= viewport.width + 40; x += 30) {
    for (let y = (viewport.offsetTop ?? 0) - 40; y <= (viewport.offsetTop ?? 0) + viewport.height + 40; y += 30) {
      for (const size of [{ width: 120, height: 80 }, { width: 320, height: 400 }, { width: 600, height: 1200 }]) {
        for (const placement of placements) {
          const pos = anchoredPopoverPosition({ left: x, right: x + 32, top: y, bottom: y + 24 }, size, viewport, placement);
          const width = Math.min(size.width, pos.maxWidth);
          assert.ok(pos.left >= left && pos.left + width <= right, `horizontal edge ${JSON.stringify({ viewport, x, y, size, placement, pos })}`);
          assert.ok(pos.top >= top && pos.top + pos.maxHeight <= bottom, `vertical edge ${JSON.stringify({ viewport, x, y, size, placement, pos })}`);
          assert.ok(pos.maxHeight > 0, "a placed popover always has room to show");
          checked++;
        }
      }
    }
  }
}

console.log(`floating position tests: passed (${checked} edge placements)`);
