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

test("writes clamp the top, overshoot the tail and skip sub-pixel corrections", () => {
  assert.equal(chatScrollWriteTarget({ ...before, scrollTop: 400 }, 1000, false), 1000);
  assert.equal(chatScrollWriteTarget(before, -10, false), 0);
  assert.equal(chatScrollWriteTarget({ ...before, scrollTop: 12.25 }, 12.25, false), null);
  assert.equal(chatScrollWriteTarget({ ...before, scrollTop: 12.25 }, 12.5, false), null);
  assert.equal(chatScrollWriteTarget({ ...before, scrollTop: 12.25 }, 13.25, false), 13.25);
  assert.equal(chatScrollWriteTarget(before, NaN, false), null);
});

test("a tail pin lands on the browser's own bottom when rounded extents sit either side of it", () => {
  const roundedUp = { scrollTop: 7286.4, scrollHeight: 7789, clientHeight: 502 };
  assert.equal(chatScrollWriteTarget(roundedUp, roundedUp.scrollHeight, false), 7789);
  const roundedDown = { scrollTop: 7287.2, scrollHeight: 7789, clientHeight: 502 };
  assert.equal(chatScrollWriteTarget(roundedDown, roundedDown.scrollHeight, false), null);
  assert.equal(chatScrollWriteTarget({ ...roundedDown, scrollTop: 7287 }, roundedDown.scrollHeight, false), null);
});

test("the bottom edge is one pixel wide on either side of the rounded extents", async () => {
  const { isChatViewportAtBottom } = await import("./chatViewport");
  for (const scrollTop of [599.2, 600, 600.8]) assert.equal(isChatViewportAtBottom({ ...before, scrollTop }), true);
  assert.equal(isChatViewportAtBottom({ ...before, scrollTop: 599 }), false);
});

test("a reader returning to a fractional bottom reattaches", () => {
  const reading = { scrollTop: 6900, scrollHeight: 7789, clientHeight: 502 };
  assert.equal(chatViewportModeAfterScroll("detached", reading, { ...reading, scrollTop: 7286.4 }, true), "pinned");
  assert.equal(chatViewportModeAfterScroll("detached", reading, { ...reading, scrollTop: 7285.6 }, true), "detached");
});

test("only a detached reader hands layout-shift compensation to the browser", async () => {
  const { chatOverflowAnchor } = await import("./chatScrollDecisions");
  assert.equal(chatOverflowAnchor("pinned"), "none");
  assert.equal(chatOverflowAnchor("detached"), "auto");
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
  assert.equal(consumes({ ...inner, scrollTop: 599.4 }, 40), false);
  assert.equal(consumes({ ...inner, scrollTop: 0.6 }, -40), false);
});
