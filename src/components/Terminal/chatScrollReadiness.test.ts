import assert from "node:assert/strict";
import { test } from "node:test";
import { isChatScrollReady } from "./chatScrollReadiness";
import { chatLayoutScrollTarget } from "./chatScrollDecisions";

test("pinned activation with rendered rows reaches bottom during a refresh", () => {
  assert.equal(isChatScrollReady("pinned", true, 10), true);
  assert.equal(chatLayoutScrollTarget("pinned", { scrollTop: 0, scrollHeight: 874, clientHeight: 719 }, null), 874);
});

test("detached activation waits for a loading refresh", () => {
  assert.equal(isChatScrollReady("detached", true, 10), false);
});

test("activation without rendered rows waits", () => {
  for (const loading of [true, false]) assert.equal(isChatScrollReady("pinned", loading, 0), false);
});

test("loaded rows keep both pinned and detached restoration ready", () => {
  assert.equal(isChatScrollReady("pinned", false, 10), true);
  assert.equal(isChatScrollReady("detached", false, 10), true);
});
