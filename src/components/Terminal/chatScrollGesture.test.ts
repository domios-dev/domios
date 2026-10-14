import assert from "node:assert/strict";
import { test } from "node:test";
import { chatScrollGestureAfterEvent, idleChatScrollGesture, isChatUserScrolling } from "./chatScrollGesture";

test("a tap blocks writes only until touchend", () => {
  const touching = chatScrollGestureAfterEvent(idleChatScrollGesture(), "touchstart");
  assert.equal(isChatUserScrolling(touching), true);
  assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(touching, "touchend")), false);
});

test("a scrolling touch retains ownership through momentum until scrollend", () => {
  let gesture = chatScrollGestureAfterEvent(idleChatScrollGesture(), "touchstart");
  gesture = chatScrollGestureAfterEvent(gesture, "scroll");
  gesture = chatScrollGestureAfterEvent(gesture, "touchend");
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll");
  assert.equal(isChatUserScrolling(gesture), true);
  assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(gesture, "scrollend")), false);
});

test("scrollend during a held touch cannot release gesture ownership", () => {
  let gesture = chatScrollGestureAfterEvent(idleChatScrollGesture(), "touchstart");
  gesture = chatScrollGestureAfterEvent(gesture, "scroll");
  gesture = chatScrollGestureAfterEvent(gesture, "scrollend");
  assert.equal(isChatUserScrolling(gesture), true);
  assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(gesture, "touchend",
    { supportsScrollEnd: true, atBottom: false, positionChanged: true })), false);
});

test("wheel scrolling owns writes until scrollend or an explicit navigation reset", () => {
  const wheel = chatScrollGestureAfterEvent(idleChatScrollGesture(), "wheel");
  assert.equal(isChatUserScrolling(wheel), true);
  assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(wheel, "scrollend")), false);
  assert.deepEqual(chatScrollGestureAfterEvent(wheel, "reset"), idleChatScrollGesture());
});

test("application scroll events do not invent a user gesture", () => {
  assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(idleChatScrollGesture(), "scroll")), false);
});

test("touchend before the queued native scroll event still retains momentum ownership", () => {
  const touching = chatScrollGestureAfterEvent(idleChatScrollGesture(), "touchstart");
  const ended = chatScrollGestureAfterEvent(touching, "touchend", { supportsScrollEnd: true, atBottom: false, positionChanged: true });
  assert.equal(isChatUserScrolling(ended), true);
  assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(ended, "scrollend")), false);
});

const withoutScrollEnd = { supportsScrollEnd: false, atBottom: false, positionChanged: true };

test("without scrollend touchend releases ownership until another native movement", () => {
  let gesture = chatScrollGestureAfterEvent(idleChatScrollGesture(), "touchstart", withoutScrollEnd);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll", withoutScrollEnd);
  gesture = chatScrollGestureAfterEvent(gesture, "touchend", withoutScrollEnd);
  assert.equal(isChatUserScrolling(gesture), false);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll", withoutScrollEnd);
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll", { ...withoutScrollEnd, atBottom: true });
  assert.equal(isChatUserScrolling(gesture), false);
});

test("without scrollend a wheel returning to the bottom resumes follow", () => {
  const wheel = chatScrollGestureAfterEvent(idleChatScrollGesture(), "wheel", withoutScrollEnd);
  const bottom = chatScrollGestureAfterEvent(wheel, "scroll", { ...withoutScrollEnd, atBottom: true });
  assert.equal(isChatUserScrolling(bottom), false);
});

test("without scrollend layout-only events cannot acquire gesture ownership", () => {
  assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(idleChatScrollGesture(), "scroll",
    { ...withoutScrollEnd, positionChanged: false })), false);
});


test("a wheel toward the bottom edge cannot block pinned arrivals while waiting for a nonexistent scrollend", () => {
  for (const supportsScrollEnd of [true, false]) {
    const input = { supportsScrollEnd, atBottom: true, positionChanged: false, towardBottom: true, canMove: false };
    assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(idleChatScrollGesture(), "wheel", input)), false);
    assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(idleChatScrollGesture(), "wheel", { ...input, towardBottom: false, canMove: true })), true);
  }
});

test("a wheel with no room in its direction never opens a gesture that only scrollend could close", () => {
  for (const supportsScrollEnd of [true, false]) {
    const input = { supportsScrollEnd, atBottom: true, positionChanged: false, towardBottom: false, canMove: false };
    assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(idleChatScrollGesture(), "wheel", input)), false);
    const moving = chatScrollGestureAfterEvent(idleChatScrollGesture(), "wheel", { ...input, canMove: true });
    assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(moving, "wheel", input)), true);
  }
});
