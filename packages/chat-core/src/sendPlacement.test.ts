import assert from "node:assert/strict";
import { test } from "node:test";
import { placePendingSends } from "./sendPlacement";
import type { NormalizedChatMessage } from "./types";

const at = 1_800_000_000_000;
const row = (id: string, delta?: number): NormalizedChatMessage => ({
  id, type: "assistant", content: id, timestamp: delta == null ? undefined : String(at + delta),
});
const send = (id: string, delta: number) => ({ id, content: id, sentAtMs: at + delta, flowAtMs: at + delta });

test("later rows push two written sends up without sorting source rows", () => {
  const sends = [send("first", 0), send("second", 1000)];
  const history = [row("old", -1000)];
  assert.deepEqual(placePendingSends(history, sends).map((item) => item.id), ["old", "first", "second"]);
  const growing = [...history, row("after-first", 500), row("after-both", 2000), row("older-stamp", -500)];
  const placed = placePendingSends(growing, sends);
  assert.deepEqual(placed.map((item) => item.id), ["old", "first", "after-first", "second", "after-both", "older-stamp"]);
  assert.deepEqual(placed.filter((item) => item.type === "assistant"), growing);
  assert.deepEqual(placePendingSends(growing, JSON.parse(JSON.stringify(sends))), placed);
});

test("absent timestamps do not fabricate placement or delivery", () => {
  const sends = [send("written", 0), { id: "held", content: "held", sentAtMs: at }];
  assert.deepEqual(placePendingSends([row("unknown"), row("later", 1000)], sends).map((item) => item.id),
    ["unknown", "written", "later", "held"]);
  assert.deepEqual(placePendingSends([row("unknown")], sends).map((item) => item.id), ["unknown", "written", "held"]);
});

test("only an explicit flow anchor places a send inside the flow", () => {
  const later = [row("old", -1), row("new", 1), row("newer", 2)];
  const unanchored = [{ id: "pending", content: "pending", sentAtMs: at }];
  assert.deepEqual(placePendingSends(later, unanchored).map((item) => item.id), ["old", "new", "newer", "pending"]);
  const anchored = [{ ...unanchored[0], flowAtMs: at }];
  assert.deepEqual(placePendingSends(later, anchored).map((item) => item.id), ["old", "pending", "new", "newer"]);
});
