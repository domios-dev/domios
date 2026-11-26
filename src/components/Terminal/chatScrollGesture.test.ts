import assert from "node:assert/strict";
import { test } from "node:test";
import {
  chatInstantScrollKey,
  chatScrollGestureAfterEvent,
  chatScrollKeyDirection,
  idleChatScrollGesture,
  isChatUserScrolling,
  isPointerOnChatScrollbar,
} from "./chatScrollGesture";

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

test("without scrollend touchend retains ownership through the next native movement", () => {
  let gesture = chatScrollGestureAfterEvent(idleChatScrollGesture(), "touchstart", withoutScrollEnd);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll", withoutScrollEnd);
  gesture = chatScrollGestureAfterEvent(gesture, "touchend", withoutScrollEnd);
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll", withoutScrollEnd);
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll", { ...withoutScrollEnd, atBottom: true });
  assert.equal(isChatUserScrolling(gesture), true);
});

test("without scrollend returning to the bottom waits for motion to stop", () => {
  const wheel = chatScrollGestureAfterEvent(idleChatScrollGesture(), "wheel", withoutScrollEnd);
  let bottom = chatScrollGestureAfterEvent(wheel, "scroll", { ...withoutScrollEnd, atBottom: true });
  assert.equal(isChatUserScrolling(bottom), true);
  for (let index = 0; index < 3; index++) bottom = chatScrollGestureAfterEvent(bottom, "frame", { ...withoutScrollEnd, positionChanged: false });
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

test("scrollbar pointer drag owns immediately, survives Chromium pointercancel, and settles on scrollend", () => {
  let gesture = chatScrollGestureAfterEvent(idleChatScrollGesture(), "pointerdown", {
    supportsScrollEnd: true, atBottom: true, positionChanged: false, onScrollbar: true,
  });
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "pointercancel");
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll");
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "scrollend");
  assert.equal(isChatUserScrolling(gesture), false);
});

test("scrollbar drag without scrollend holds ownership while pressed and settles after release", () => {
  let gesture = chatScrollGestureAfterEvent(idleChatScrollGesture(), "pointerdown", { ...withoutScrollEnd, onScrollbar: true });
  gesture = chatScrollGestureAfterEvent(gesture, "scroll", withoutScrollEnd);
  for (let index = 0; index < 5; index++) {
    gesture = chatScrollGestureAfterEvent(gesture, "frame", { ...withoutScrollEnd, positionChanged: false });
    assert.equal(isChatUserScrolling(gesture), true, "stationary frames cannot release a held scrollbar drag");
  }
  gesture = chatScrollGestureAfterEvent(gesture, "pointerup", withoutScrollEnd);
  assert.equal(isChatUserScrolling(gesture), true);
  for (let index = 0; index < 3; index++) {
    gesture = chatScrollGestureAfterEvent(gesture, "frame", { ...withoutScrollEnd, positionChanged: false });
  }
  assert.equal(isChatUserScrolling(gesture), false);
});

test("plain content click releases immediately on pointerup, while content drag waits for pointerup and scrollend", () => {
  for (const supportsScrollEnd of [true, false]) {
    const pressed = chatScrollGestureAfterEvent(idleChatScrollGesture(), "pointerdown", {
      supportsScrollEnd, atBottom: false, positionChanged: false, onScrollbar: false,
    });
    assert.equal(isChatUserScrolling(pressed), true);
    const clicked = chatScrollGestureAfterEvent(pressed, "pointerup", {
      supportsScrollEnd, atBottom: false, positionChanged: false,
    });
    assert.equal(isChatUserScrolling(clicked), false);
  }

  let drag = chatScrollGestureAfterEvent(idleChatScrollGesture(), "pointerdown", {
    supportsScrollEnd: true, atBottom: false, positionChanged: false, onScrollbar: false,
  });
  drag = chatScrollGestureAfterEvent(drag, "scroll");
  drag = chatScrollGestureAfterEvent(drag, "scrollend");
  assert.equal(isChatUserScrolling(drag), true, "scrollend during a held content pointer drag keeps ownership");
  drag = chatScrollGestureAfterEvent(drag, "pointerup", { supportsScrollEnd: true, atBottom: false, positionChanged: true });
  assert.equal(isChatUserScrolling(drag), false);
});

test("keyboard scroll keys own until scrollend or quiescence and ignore blocked directions", () => {
  assert.equal(chatScrollKeyDirection("PageUp"), -1);
  assert.equal(chatScrollKeyDirection("Home"), -1);
  assert.equal(chatScrollKeyDirection("ArrowUp"), -1);
  assert.equal(chatScrollKeyDirection("Space", true), -1);
  assert.equal(chatScrollKeyDirection("PageDown"), 1);
  assert.equal(chatScrollKeyDirection("End"), 1);
  assert.equal(chatScrollKeyDirection("ArrowDown"), 1);
  assert.equal(chatScrollKeyDirection("Space", false), 1);
  assert.equal(chatScrollKeyDirection("Enter"), 0);

  const blocked = chatScrollGestureAfterEvent(idleChatScrollGesture(), "keydown", {
    supportsScrollEnd: true, atBottom: false, positionChanged: false, towardBottom: false, canMove: false,
  });
  assert.equal(isChatUserScrolling(blocked), false);

  const active = chatScrollGestureAfterEvent(idleChatScrollGesture(), "keydown", {
    supportsScrollEnd: true, atBottom: false, positionChanged: false, towardBottom: false, canMove: true,
  });
  assert.equal(isChatUserScrolling(active), true);
  assert.equal(isChatUserScrolling(chatScrollGestureAfterEvent(active, "scrollend")), false);
});

test("scrollbar hit detection recognises classic gutters, RTL gutters, and overlay edges", () => {
  const rect = { left: 100, right: 900, top: 40, bottom: 740 };
  const classic = { clientLeft: 0, clientTop: 0, clientWidth: 792, clientHeight: 700, offsetWidth: 800, offsetHeight: 700 };
  assert.equal(isPointerOnChatScrollbar(rect, classic, { clientX: 896, clientY: 200 }), true);
  assert.equal(isPointerOnChatScrollbar(rect, classic, { clientX: 500, clientY: 200 }), false);

  const rtl = { clientLeft: 8, clientTop: 0, clientWidth: 792, clientHeight: 700, offsetWidth: 800, offsetHeight: 700 };
  assert.equal(isPointerOnChatScrollbar(rect, rtl, { clientX: 104, clientY: 200 }), true);

  const overlay = { clientLeft: 0, clientTop: 0, clientWidth: 800, clientHeight: 700, offsetWidth: 800, offsetHeight: 700 };
  assert.equal(isPointerOnChatScrollbar(rect, overlay, { clientX: 892, clientY: 200 }), true);
  assert.equal(isPointerOnChatScrollbar(rect, overlay, { clientX: 850, clientY: 200 }), false);
});

test("only unmodified Home and End classify as instant virtualized log jumps", () => {
  assert.equal(chatInstantScrollKey("Home"), "top");
  assert.equal(chatInstantScrollKey("End"), "bottom");
  assert.equal(chatInstantScrollKey("Home", true), null);
  assert.equal(chatInstantScrollKey("End", true), null);
  assert.equal(chatInstantScrollKey("PageUp"), null);
  assert.equal(chatInstantScrollKey("PageDown"), null);
  assert.equal(chatInstantScrollKey("ArrowUp"), null);
  assert.equal(chatInstantScrollKey("Space"), null);
});
