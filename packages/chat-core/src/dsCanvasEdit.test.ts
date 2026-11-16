import assert from "node:assert/strict";
import test from "node:test";
import { parseDsCanvas } from "./dsCanvas";
import { applyDsCanvasEdit, invertDsCanvasEdit, nextDsCanvasNoteId, reconcileDsCanvas, type DsCanvasEdit } from "./dsCanvasEdit";

const board = JSON.stringify({
  title: "Board",
  items: [
    { kind: "image", src: "/work/a.png", x: 0, y: 0, w: 640, h: 400 },
    { id: "n", kind: "note", markdown: "agent" },
  ],
  views: [{ name: "all", x: 0, y: 0, w: 640, h: 400 }],
});
const items = (text: string) => (JSON.parse(text) as { items: Record<string, unknown>[] }).items;
const canvas = (text: string) => {
  const parsed = parseDsCanvas(text);
  assert.ok(parsed.ok);
  return parsed.canvas;
};

test("edits address items by id, pin the ids the board shows, and keep the rest of the file", () => {
  const moved = applyDsCanvasEdit(board, { kind: "frame", id: "item-1", frame: { x: 40.4, y: -9.6 } });
  assert.ok(moved);
  assert.deepEqual(items(moved)[0], { id: "item-1", kind: "image", src: "/work/a.png", x: 40, y: -10, w: 640, h: 400 });
  assert.deepEqual(JSON.parse(moved).views, JSON.parse(board).views);
  assert.equal(JSON.parse(moved).title, "Board");
  const flowed = applyDsCanvasEdit(moved, { kind: "frame", id: "item-1", frame: { x: null, y: null } });
  assert.deepEqual(items(flowed!)[0], { id: "item-1", kind: "image", src: "/work/a.png", w: 640, h: 400 });
  assert.equal(items(applyDsCanvasEdit(board, { kind: "text", id: "n", markdown: "**owner**" })!)[1].markdown, "**owner**");
  assert.equal(applyDsCanvasEdit(board, { kind: "text", id: "item-1", markdown: "x" }), null, "only notes take text");
});

test("insert, delete and the refusals that keep a canvas valid", () => {
  const note = { id: "note-1", kind: "note", markdown: "", x: 10, y: 20, w: 320, h: 180 };
  const added = applyDsCanvasEdit(board, { kind: "insert", index: 2, item: note });
  assert.deepEqual(items(added!).map((item) => item.id), ["item-1", "n", "note-1"]);
  assert.equal(applyDsCanvasEdit(added!, { kind: "insert", index: 0, item: note }), null, "ids stay unique");
  const removed = applyDsCanvasEdit(board, { kind: "delete", id: "n" });
  assert.deepEqual(items(removed!).map((item) => item.id), ["item-1"]);
  assert.equal(applyDsCanvasEdit(removed!, { kind: "delete", id: "item-1" }), null, "the last item stays");
  assert.equal(applyDsCanvasEdit(board, { kind: "delete", id: "gone" }), null);
  assert.equal(applyDsCanvasEdit("{", { kind: "delete", id: "n" }), null);
  assert.equal(applyDsCanvasEdit(board, { kind: "frame", id: "n", frame: { w: 0 } }), null, "invalid sizes never reach the file");
});

test("every edit has an inverse that restores the board", () => {
  const edits: DsCanvasEdit[] = [
    { kind: "frame", id: "item-1", frame: { x: 5, y: 6 } },
    { kind: "frame", id: "n", frame: { x: 5, y: 6, w: 200, h: 100 } },
    { kind: "text", id: "n", markdown: "new" },
    { kind: "delete", id: "item-1" },
    { kind: "insert", index: 1, item: { id: "note-1", kind: "note", markdown: "x" } },
  ];
  const pinned = applyDsCanvasEdit(board, { kind: "frame", id: "n", frame: {} })!;
  for (const edit of edits) {
    const inverse = invertDsCanvasEdit(pinned, edit);
    assert.ok(inverse, edit.kind);
    assert.equal(applyDsCanvasEdit(applyDsCanvasEdit(pinned, edit)!, inverse), pinned, edit.kind);
  }
});

test("a reload reuses unchanged item objects and the whole board when nothing changed", () => {
  const before = canvas(board);
  assert.equal(reconcileDsCanvas(before, canvas(board)), before);
  const after = reconcileDsCanvas(before, canvas(applyDsCanvasEdit(board, { kind: "text", id: "n", markdown: "changed" })!));
  assert.notEqual(after, before);
  assert.equal(after.items[0], before.items[0]);
  assert.notEqual(after.items[1], before.items[1]);
  assert.equal(after.views, before.views);
  assert.equal(nextDsCanvasNoteId(canvas(JSON.stringify({ items: [{ id: "note-1", kind: "note", markdown: "" }] }))), "note-2");
});
