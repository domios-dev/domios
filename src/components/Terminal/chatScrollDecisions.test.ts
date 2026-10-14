import assert from "node:assert/strict";
import { test } from "node:test";
import { chatLayoutScrollTarget, chatScrollWriteTarget, shouldCaptureChatAnchor } from "./chatScrollDecisions";
import { chatViewportModeAfterScroll } from "./chatViewport";

const before = { scrollTop: 600, scrollHeight: 1000, clientHeight: 400 };

test("user movement concurrent with an incoming row detaches instead of targeting the tail", () => {
  const arriving = { ...before, scrollTop: 550, scrollHeight: 1200 };
  const mode = chatViewportModeAfterScroll("pinned", before, arriving, true);
  assert.equal(mode, "detached");
  assert.equal(chatLayoutScrollTarget(mode, arriving, null), 550);
});

test("concurrent incoming layout captures the user's new reader anchor", () => {
  assert.equal(shouldCaptureChatAnchor(before, { ...before, scrollTop: 450, scrollHeight: 1200 }, true), true);
});

test("tail repair avoids assigning the same effective clamped position", () => {
  assert.equal(chatScrollWriteTarget(before, before.scrollHeight, false), null);
});

test("an active gesture suppresses a script write even when the requested position differs", () => {
  assert.equal(chatScrollWriteTarget({ ...before, scrollTop: 400 }, before.scrollHeight, true), null);
});

test("layout clamping during a touch does not invent movement or reattach a reader", () => {
  const clamped = { scrollTop: 500, scrollHeight: 900, clientHeight: 400 };
  assert.equal(chatViewportModeAfterScroll("pinned", before, clamped, true), "pinned");
  assert.equal(chatViewportModeAfterScroll("detached", before, clamped, true), "detached");
  assert.equal(shouldCaptureChatAnchor(before, clamped, true), false);
});

test("layout growth without user movement preserves follow authority", () => {
  const arriving = { ...before, scrollHeight: 1200 };
  assert.equal(chatViewportModeAfterScroll("pinned", before, arriving, true), "pinned");
  assert.equal(shouldCaptureChatAnchor(before, arriving, true), false);
});

test("an active reader returning to the bottom can reattach despite a height change", () => {
  const arriving = { ...before, scrollTop: 800, scrollHeight: 1200 };
  assert.equal(chatViewportModeAfterScroll("detached", { ...before, scrollTop: 450 }, arriving, true), "pinned");
});

test("writes clamp both ends and retain fractional positions", () => {
  assert.equal(chatScrollWriteTarget({ ...before, scrollTop: 400 }, 1000, false), 600);
  assert.equal(chatScrollWriteTarget(before, -10, false), 0);
  assert.equal(chatScrollWriteTarget({ ...before, scrollTop: 12.25 }, 12.25, false), null);
  assert.equal(chatScrollWriteTarget({ ...before, scrollTop: 12.25 }, 12.5, false), 12.5);
  assert.equal(chatScrollWriteTarget(before, NaN, false), null);
});

test("native out-of-range overscroll is not captured as a new reader anchor", () => {
  assert.equal(shouldCaptureChatAnchor(before, { ...before, scrollTop: -15, scrollHeight: 1200 }, true), false);
});

test("inner wheel consumption transfers to the transcript at either directional boundary", async () => {
  const { chatInnerScrollConsumesWheel: consumes } = await import("./chatScrollDecisions");
  const inner = { scrollTop: 0, scrollHeight: 800, clientHeight: 200 };
  assert.equal(consumes(inner, -40), false);
  assert.equal(consumes(inner, 40), true);
  assert.equal(consumes({ ...inner, scrollTop: 600 }, 40), false);
  assert.equal(consumes({ ...inner, scrollTop: 600 }, -40), true);
  assert.equal(consumes({ ...inner, scrollTop: 250.5 }, 0.25), true);
  assert.equal(consumes({ ...inner, scrollHeight: 200 }, 40), false);
  assert.equal(consumes({ ...inner, scrollTop: -10 }, -40), false);
  assert.equal(consumes({ ...inner, scrollTop: 620 }, 40), false);
  assert.equal(consumes(inner, 0), false);
  assert.equal(consumes(inner, NaN), false);
});
