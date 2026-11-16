import assert from "node:assert/strict";
import test from "node:test";
import { parseDsCanvas, type DsCanvas } from "@codeterm/chat-core";
import { CANVAS_LIVE_FRAME_BUDGET, canvasPagerLabel, canvasPagerTargets, chooseLiveFrames, fitPreview, layoutCanvas, sentenceCase, stepCanvasPager } from "./canvasBlockModel";

function canvas(document: unknown): DsCanvas {
  const parsed = parseDsCanvas(JSON.stringify(document));
  assert.ok(parsed.ok, parsed.ok ? "" : parsed.reason);
  return parsed.canvas;
}

test("placed items keep their frame and the rest flow in wrapped rows below them", () => {
  const layout = layoutCanvas(canvas({ items: [
    { kind: "image", src: "/a.png", x: 100, y: 50, w: 640, h: 400 },
    { kind: "note", markdown: "n", w: 900, h: 100 },
    { kind: "note", markdown: "n", w: 900, h: 200 },
    { kind: "html", html: "<b>x</b>", x: 10 },
  ] }));
  assert.deepEqual(layout.items.map(({ x, y, w, h }) => [x, y, w, h]), [[100, 50, 640, 400], [100, 490, 900, 100], [100, 630, 900, 200], [1040, 630, 480, 360]]);
  assert.deepEqual(layout.bounds, { x: 100, y: 50, width: 1420, height: 940 });
});

test("the pager steps through views, or items without views, and returns to the whole canvas", () => {
  const withViews = canvas({ items: [{ kind: "note", markdown: "a" }], views: [{ name: "A", x: 0, y: 0, w: 10, h: 10 }, { name: "B", x: 20, y: 0, w: 10, h: 10 }] });
  assert.deepEqual(canvasPagerTargets(withViews, layoutCanvas(withViews)).map((target) => target.label), ["A", "B"]);
  assert.deepEqual(["compare", "side_by-side", "Before and After"].map(sentenceCase), ["Compare", "Side by side", "Before and After"]);
  const items = canvas({ items: [{ kind: "note", markdown: "a", title: "First" }, { kind: "image", src: "C:\\shots\\after.png" }] });
  assert.deepEqual(canvasPagerTargets(items, layoutCanvas(items)).map((target) => target.label), ["First", "after.png"]);
  const walk = [null as number | null];
  for (let step = 0; step < 4; step++) walk.push(stepCanvasPager(walk.at(-1)!, 3, 1));
  assert.deepEqual(walk, [null, 0, 1, 2, null]);
  assert.equal(stepCanvasPager(null, 3, -1), 2);
  assert.equal(stepCanvasPager(0, 3, -1), null);
  assert.equal(stepCanvasPager(null, 0, 1), null);
  assert.deepEqual([canvasPagerLabel(null, 3), canvasPagerLabel(1, 3)], ["All", "2 / 3"]);
});

test("live frames stay within the budget, selected first, then visible ones in order", () => {
  const frames = Array.from({ length: 20 }, (_, index) => ({ id: `f${index}`, visible: index % 2 === 0 }));
  const live = chooseLiveFrames(frames, null);
  assert.equal(live.size, CANVAS_LIVE_FRAME_BUDGET);
  assert.ok([...live].every((id) => Number(id.slice(1)) % 2 === 0));
  const selected = chooseLiveFrames(frames, "f19");
  assert.equal(selected.size, CANVAS_LIVE_FRAME_BUDGET);
  assert.ok(selected.has("f19"), "a selected frame is live even when it is hidden and the budget is full");
  assert.equal(chooseLiveFrames(frames.map((frame) => ({ ...frame, visible: false })), null).size, 0, "nothing off screen loads");
  assert.equal(chooseLiveFrames(frames, "not-a-frame").size, CANVAS_LIVE_FRAME_BUDGET);
});

test("the static preview fits and centres the whole canvas without enlarging it", () => {
  assert.deepEqual(fitPreview({ x: 0, y: 0, width: 1600, height: 400 }, 800, 320), { scale: 0.5, x: 0, y: 60 });
  assert.deepEqual(fitPreview({ x: -100, y: 0, width: 200, height: 100 }, 800, 320), { scale: 1, x: 400, y: 110 });
  assert.equal(fitPreview({ x: 0, y: 0, width: 10, height: 10 }, 0, 320).scale, 0);
});
