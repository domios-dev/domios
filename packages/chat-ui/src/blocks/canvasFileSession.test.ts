import assert from "node:assert/strict";
import test from "node:test";
import { CanvasFileSession } from "./canvasFileSession";
import type { CanvasWriteResult } from "./canvasFilePort";

const board = (note: string, x = 0) => JSON.stringify({ format: "domios.canvas/1", items: [
  { id: "img", kind: "image", src: "/work/a.png", x, y: 0, w: 640, h: 400 },
  { id: "n", kind: "note", markdown: note },
] });
const field = (text: string | null, id: string, key: string) => (JSON.parse(text!) as { items: Record<string, unknown>[] }).items.find((item) => item.id === id)?.[key];

/** A file on disk with an injected write: CAS on the exact text, as the native write route does. */
function disk(initial: string) {
  const state = { text: initial, writes: [] as string[], fail: null as string | null, reloads: 0, gate: null as Promise<void> | null };
  const session = new CanvasFileSession({
    async write(base, next): Promise<CanvasWriteResult> {
      if (state.gate) await state.gate;
      if (state.fail) throw new Error(state.fail);
      if (base !== state.text) return { status: "conflict", current: state.text };
      state.text = next;
      state.writes.push(next);
      return { status: "written" };
    },
    reload: () => { state.reloads++; },
  });
  session.observe(initial);
  return { state, session };
}
const settle = async () => { for (let index = 0; index < 10; index++) await Promise.resolve(); };

test("an edit on a file the agent changed underneath lands on top of the agent's change", async () => {
  const { state, session } = disk(board("draft"));
  state.text = board("agent rewrote this");
  assert.ok(session.commit({ kind: "frame", id: "img", frame: { x: 120 } }));
  assert.equal(field(session.snapshot().text, "img", "x"), 120, "the owner sees the move at once");
  await settle();
  assert.equal(state.writes.length, 1);
  assert.equal(field(state.text, "n", "markdown"), "agent rewrote this");
  assert.equal(field(state.text, "img", "x"), 120);
  assert.equal(session.snapshot().text, state.text);
  assert.equal(session.snapshot().failure, null);
});

test("a failed save keeps the change in memory until Try again saves it", async () => {
  const { state, session } = disk(board("a"));
  state.fail = "Preview unavailable";
  session.commit({ kind: "text", id: "n", markdown: "owner" });
  await settle();
  assert.equal(session.snapshot().failure, "Preview unavailable");
  assert.equal(field(session.snapshot().text, "n", "markdown"), "owner");
  assert.equal(field(state.text, "n", "markdown"), "a");
  state.fail = null;
  session.retry();
  await settle();
  assert.equal(session.snapshot().failure, null);
  assert.equal(field(state.text, "n", "markdown"), "owner");
});

test("file updates wait for the gesture, then apply under the owner's committed edit", async () => {
  const { state, session } = disk(board("a"));
  const release = session.hold();
  session.observe(board("agent", 0));
  assert.equal(field(session.snapshot().text, "n", "markdown"), "a", "nothing moves under the pointer");
  state.text = board("agent", 0);
  session.commit({ kind: "text", id: "n", markdown: "owner wins" });
  release();
  await settle();
  assert.equal(field(state.text, "n", "markdown"), "owner wins");
  assert.equal(session.snapshot().text, state.text);
});

test("reads that race a write are dropped and the file is read again once it lands", async () => {
  const { state, session } = disk(board("a"));
  let open!: () => void;
  state.gate = new Promise((resolve) => { open = resolve; });
  session.commit({ kind: "frame", id: "img", frame: { x: 50 } });
  await settle();
  session.observe(board("a"));
  assert.equal(field(session.snapshot().text, "img", "x"), 50, "a stale read never undoes the move on screen");
  state.gate = null;
  open();
  await settle();
  assert.equal(state.reloads, 1);
  assert.equal(field(state.text, "img", "x"), 50);
});

test("undo and redo step through move, delete and add without a confirm", async () => {
  const { state, session } = disk(board("a"));
  session.commit({ kind: "delete", id: "img" });
  session.commit({ kind: "insert", index: 1, item: { id: "note-1", kind: "note", markdown: "" } });
  await settle();
  assert.equal(session.commit({ kind: "delete", id: "missing" }), false);
  assert.ok(session.undo());
  assert.ok(session.undo());
  await settle();
  assert.deepEqual((JSON.parse(state.text) as { items: { id: string }[] }).items.map((item) => item.id), ["img", "n"]);
  assert.equal(session.snapshot().canRedo, true);
  assert.ok(session.redo());
  await settle();
  assert.deepEqual((JSON.parse(state.text) as { items: { id: string }[] }).items.map((item) => item.id), ["n"]);
  assert.equal(session.commit({ kind: "delete", id: "n" }), false, "the last item cannot be deleted");
});
