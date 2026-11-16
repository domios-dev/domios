import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { parseDsBlocks } from "./dsBlocks";
import { dsCanvasFilePaths, parseDsCanvas } from "./dsCanvas";

const corpus = JSON.parse(readFileSync(new URL("./dsBlocks.fixtures.json", import.meta.url), "utf8")) as { fixtures: { name: string; expectedBlocks: { block: { kind: string; body?: string }; paths?: string[] }[] }[] };

test("canvas fixtures name exactly the file paths the native preview guard admits", () => {
  const canvases = corpus.fixtures.flatMap((fixture) => fixture.expectedBlocks.filter((span) => span.block.kind === "canvas").map((span) => ({ name: fixture.name, span })));
  assert.ok(canvases.length >= 5);
  for (const { name, span } of canvases) assert.deepEqual(dsCanvasFilePaths(span.block.body!), span.paths, name);
});

test("a canvas reads items with optional frames, default ids and named views", () => {
  const parsed = parseDsCanvas(JSON.stringify({
    title: "Options",
    items: [
      { kind: "image", src: "/work/a.png", x: 0, y: 0, w: 640, h: 400 },
      { id: "mock", kind: "html", html: "<b>B</b>", title: "Mock" },
      { kind: "note", markdown: "**A** wins" },
    ],
    views: [{ name: "compare", x: 0, y: 0, w: 1160, h: 400 }],
  }));
  assert.ok(parsed.ok);
  assert.equal(parsed.canvas.title, "Options");
  assert.deepEqual(parsed.canvas.items.map((item) => [item.id, item.kind]), [["item-1", "image"], ["mock", "html"], ["item-3", "note"]]);
  assert.deepEqual(parsed.canvas.items[0], { id: "item-1", kind: "image", src: "/work/a.png", x: 0, y: 0, w: 640, h: 400 });
  assert.deepEqual(parsed.canvas.views, [{ name: "compare", x: 0, y: 0, w: 1160, h: 400 }]);
});

test("an unreadable canvas explains itself in one sentence", () => {
  const cases: [string, RegExp][] = [
    ["{\"items\":[}", /^The JSON is invalid: /],
    ["{\"items\":[{\"kind\":\"note\",\"markdown\":\"\\ud800\"}]}", /unpaired surrogate/],
    ["[]", /must be a JSON object/],
    ["{\"items\":[]}", /needs 1 to 64 items/],
    ["{\"items\":[{\"kind\":\"image\"}]}", /Item 1 \(image\) needs "src" text/],
    ["{\"items\":[{\"kind\":\"html\",\"html\":\"x\",\"src\":\"/a\"}]}", /Item 1 has an unknown field "src"/],
    ["{\"items\":[{\"kind\":\"note\",\"markdown\":\"x\",\"x\":\"1\"}]}", /Item 1 has an invalid x/],
    ["{\"items\":[{\"kind\":\"note\",\"markdown\":\"x\",\"h\":-4}]}", /Item 1 has an invalid h/],
    ["{\"items\":[{\"kind\":\"note\",\"markdown\":\"x\"}],\"views\":[{\"name\":\"a\",\"x\":0,\"y\":0,\"w\":1}]}", /View 1 needs x, y and positive w and h/],
    ["{\"items\":[{\"kind\":\"note\",\"markdown\":\"x\"}],\"layout\":\"grid\"}", /unknown field "layout"/],
  ];
  for (const [body, reason] of cases) {
    const parsed = parseDsCanvas(body);
    assert.equal(parsed.ok, false, body);
    if (!parsed.ok) assert.match(parsed.reason, reason, body);
  }
});

test("a canvas keeps the ds-html byte bound and the 32-file preview bound", () => {
  const oversized = parseDsCanvas(JSON.stringify({ items: [{ kind: "html", html: "x".repeat(200_000) }] }));
  assert.equal(oversized.ok, false);
  const files = (count: number) => JSON.stringify({ items: Array.from({ length: count }, (_, index) => ({ kind: "image", src: `/work/${index}.png` })) });
  assert.equal(parseDsCanvas(files(32)).ok, true);
  assert.equal(parseDsCanvas(files(33)).ok, false);
  const [segment] = parseDsBlocks(`<ds-canvas>${files(33)}</ds-canvas>`);
  assert.equal(segment.type, "block", "an over-limit canvas still renders its explanation");
});
