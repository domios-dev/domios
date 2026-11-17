import assert from "node:assert/strict";
import test from "node:test";
import { parseDsCanvasFile, type DsCanvas } from "@codeterm/chat-core";
import { CANVAS_LIVE_FRAME_BUDGET, canvasPagerLabel, canvasPagerTargets, canvasViewTarget, chooseLiveFrames, fitPreview, layoutCanvas, noteLabel, noteSpot, reconcilePager, resizeFrame, sentenceCase, stepCanvasPager } from "./canvasBlockModel";
import { gestureEdit } from "./canvasEditing";

function canvas(document: unknown): DsCanvas {
  const parsed = parseDsCanvasFile(JSON.stringify({ format: "domios.canvas/1", ...document as object }));
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

test("the static preview fits and centres the whole canvas inside its padding without enlarging it", () => {
  assert.deepEqual(fitPreview({ x: 0, y: 0, width: 1600, height: 400 }, 800, 320, 0), { scale: 0.5, x: 0, y: 60 });
  assert.deepEqual(fitPreview({ x: 0, y: 0, width: 1536, height: 400 }, 800, 320), { scale: 0.5, x: 16, y: 60 }, "16px padding on every side by default");
  assert.deepEqual(fitPreview({ x: -100, y: 0, width: 200, height: 100 }, 800, 320), { scale: 1, x: 400, y: 110 });
  assert.equal(fitPreview({ x: 0, y: 0, width: 10, height: 10 }, 0, 320).scale, 0);
  assert.equal(fitPreview({ x: 0, y: 0, width: 10, height: 10 }, 32, 320).scale, 0, "no room inside the padding");
});

test("resizing keeps the 120x80 minimum and the aspect ratio of images only", () => {
  assert.deepEqual(resizeFrame({ w: 320, h: 180 }, 40, -500, false), { w: 360, h: 80 });
  assert.deepEqual(resizeFrame({ w: 400, h: 200 }, 100, 0, true), { w: 500, h: 250 });
  assert.deepEqual(resizeFrame({ w: 400, h: 200 }, 0, 50, true), { w: 500, h: 250 });
  assert.deepEqual(resizeFrame({ w: 400, h: 100 }, -1000, 0, true), { w: 320, h: 80 }, "a wide image stops at the minimum height");
  assert.deepEqual(resizeFrame({ w: 100, h: 400 }, -1000, 0, true), { w: 120, h: 480 }, "a tall image stops at the minimum width");
});

test("a new note goes to the view centre and steps aside from a note already there", () => {
  const placed = layoutCanvas(canvas({ items: [{ id: "a", kind: "note", markdown: "", x: 340, y: 210 }, { id: "b", kind: "image", src: "/a.png", x: 364, y: 234 }] })).items;
  assert.deepEqual(noteSpot({ x: 500, y: 300 }, placed), { x: 364, y: 234 }, "only notes push a new note aside");
  assert.deepEqual(noteSpot({ x: 0, y: 0 }, placed), { x: -160, y: -90 });
});

test("the pager keeps its view or item by identity and clamps when it disappears", () => {
  const target = (key: string) => ({ key, label: key, bounds: { x: 0, y: 0, width: 1, height: 1 } });
  const targets = ["item:a", "item:b", "item:c"].map(target);
  assert.equal(reconcilePager(targets, { key: "item:c", index: 2 }), 2);
  assert.equal(reconcilePager([target("item:c"), target("item:a")], { key: "item:c", index: 2 }), 0, "a moved target is followed");
  assert.equal(reconcilePager([target("item:a"), target("item:b")], { key: "item:c", index: 2 }), 1, "a removed target clamps");
  assert.equal(reconcilePager([], { key: "item:c", index: 2 }), null);
  assert.equal(reconcilePager(targets, null), null);
  const items = canvas({ items: [{ id: "x", kind: "note", markdown: "a" }], views: [{ name: "v", x: 0, y: 0, w: 1, h: 1 }] });
  assert.deepEqual(canvasPagerTargets(items, layoutCanvas(items)).map((entry) => entry.key), ["view:v"]);
});

test("a released gesture commits its frame from its own start; a press below the threshold commits nothing", () => {
  const gesture = { id: "a", mode: "move" as const, startX: 100, startY: 100, scale: 2, start: { x: 10, y: 20, w: 400, h: 200 }, keepAspect: false, moved: true, release: null };
  assert.deepEqual(gestureEdit(gesture, 300, 50), { kind: "frame", id: "a", frame: { x: 110, y: -5 } });
  assert.deepEqual(gestureEdit({ ...gesture, mode: "resize", keepAspect: true }, 300, 100), { kind: "frame", id: "a", frame: { w: 500, h: 250 } });
  assert.equal(gestureEdit({ ...gesture, moved: false }, 300, 50), null);
});

test("a note is named by its first line of text", () => {
  assert.equal(noteLabel("\n## **Ship** the `calm` rows\nmore"), "Ship the calm rows");
  assert.equal(noteLabel("- first point\n- second"), "first point");
  assert.equal(noteLabel("   \n"), "Note");
  assert.equal(noteLabel("x".repeat(60)), `${"x".repeat(39)}…`);
});

test("a named view resolves to its pager index and rectangle", () => {
  const board = canvas({ items: [{ kind: "note", markdown: "x" }], views: [{ name: "a", x: 0, y: 0, w: 10, h: 10 }, { name: "b", x: 5, y: 6, w: 7, h: 8 }] });
  assert.deepEqual(canvasViewTarget(board, "b"), { index: 1, bounds: { x: 5, y: 6, width: 7, height: 8 } });
  assert.equal(canvasViewTarget(board, "missing"), null);
  assert.equal(canvasViewTarget(board, undefined), null);
});
