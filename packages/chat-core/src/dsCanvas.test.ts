import assert from "node:assert/strict";
import test from "node:test";
import { DS_CANVAS_FILE_FORMAT, isDsCanvasFilePath, parseDsCanvasFile } from "./dsCanvas";

const file = (document: unknown) => JSON.stringify({ format: DS_CANVAS_FILE_FORMAT, ...(document as Record<string, unknown>) });

test("a canvas file reads items with optional frames, default ids and named views", () => {
  const parsed = parseDsCanvasFile(file({
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

test("an unreadable canvas file explains itself in one sentence", () => {
  const cases: [string, RegExp][] = [
    ["{\"format\":\"domios.canvas/1\",\"items\":[}", /^The JSON is invalid: /],
    ["{\"format\":\"domios.canvas/1\",\"items\":[{\"kind\":\"note\",\"markdown\":\"\\ud800\"}]}", /unpaired surrogate/],
    [file({ items: [] }), /needs 1 to 64 items/],
    [file({ items: [{ kind: "image" }] }), /Item 1 \(image\) needs "src" text/],
    [file({ items: [{ kind: "html", html: "x", src: "/a" }] }), /Item 1 has an unknown field "src"/],
    [file({ items: [{ kind: "note", markdown: "x", x: "1" }] }), /Item 1 has an invalid x/],
    [file({ items: [{ kind: "note", markdown: "x", h: -4 }] }), /Item 1 has an invalid h/],
    [file({ items: [{ kind: "note", markdown: "x" }], views: [{ name: "a", x: 0, y: 0, w: 1 }] }), /View 1 needs x, y and positive w and h/],
    [file({ items: [{ kind: "note", markdown: "x" }], layout: "grid" }), /unknown field "layout"/],
  ];
  for (const [text, reason] of cases) {
    const parsed = parseDsCanvasFile(text);
    assert.equal(parsed.ok, false, text);
    if (!parsed.ok) assert.match(parsed.reason, reason, text);
  }
});

test("canvas files keep the ds-html byte bound and the 32-file item bound", () => {
  const oversized = parseDsCanvasFile(file({ items: [{ kind: "html", html: "x".repeat(200_000) }] }));
  assert.equal(oversized.ok, false);
  const files = (count: number) => file({ items: Array.from({ length: count }, (_, index) => ({ kind: "image", src: `/work/${index}.png` })) });
  assert.equal(parseDsCanvasFile(files(32)).ok, true);
  assert.equal(parseDsCanvasFile(files(33)).ok, false);
});

test("a canvas file requires its format header first and a known major version", () => {
  const items = `"items":[{"kind":"image","src":"/work/a.png"}]`;
  assert.ok(parseDsCanvasFile(`\n{ "format": "${DS_CANVAS_FILE_FORMAT}", ${items} }`).ok);
  assert.ok(parseDsCanvasFile(`{"format":"domios.canvas/1.4",${items}}`).ok, "a minor version still reads");
  for (const [text, reason] of [
    [`{${items}}`, /first field/],
    [`{${items},"format":"domios.canvas/1"}`, /first field/],
    [`{"format":1,${items}}`, /first field/],
    [`{"format":"domios.canvas/2",${items}}`, /"domios\.canvas\/2", which this version cannot read/],
    [`{"format":"domios.canvas/1.",${items}}`, /cannot read/],
    [`{"format":"domios.canvas/1","extra":true,${items}}`, /unknown field "extra"/],
  ] as const) {
    const parsed = parseDsCanvasFile(text);
    assert.ok(!parsed.ok && reason.test(parsed.reason), text);
  }
  for (const path of ["/work/board.canvas.json", "C:\\work\\Board.Canvas.JSON"]) assert.ok(isDsCanvasFilePath(path), path);
  for (const path of ["/work/board.canvas", "/work/board.json", "/work/.canvas.json", "/work/board.dscanvas"]) assert.ok(!isDsCanvasFilePath(path), path);
});
