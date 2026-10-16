import assert from "node:assert/strict";
import { test } from "node:test";
import { captureChatAnchor } from "./chatAnchorModel";
import { chatScrollGestureAfterEvent, idleChatScrollGesture, isChatUserScrolling } from "./chatScrollGesture";
import { chatShiftAnchorAt, chatSpacerAfterLayout, chatSpacerFold, chatSpacerHeight, chatSpacerTranslation, chatLayoutViewportTop } from "./chatSpacerDrift";
import { chatScrollWriteTarget } from "./chatScrollDecisions";
import { mountedChatWindow } from "./chatMountedWindow";

test("touch release without scrollend keeps momentum ownership", () => {
  const input = { supportsScrollEnd: false, atBottom: false, positionChanged: true };
  let gesture = chatScrollGestureAfterEvent(idleChatScrollGesture(), "touchstart", input);
  gesture = chatScrollGestureAfterEvent(gesture, "scroll", input);
  gesture = chatScrollGestureAfterEvent(gesture, "touchend", input);
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "frame", { ...input, positionChanged: false });
  gesture = chatScrollGestureAfterEvent(gesture, "frame", { ...input, positionChanged: false });
  assert.equal(isChatUserScrolling(gesture), true);
  gesture = chatScrollGestureAfterEvent(gesture, "frame", input);
  for (let index = 0; index < 2; index++) {
    gesture = chatScrollGestureAfterEvent(gesture, "frame", { ...input, positionChanged: false });
    assert.equal(isChatUserScrolling(gesture), true);
  }
  gesture = chatScrollGestureAfterEvent(gesture, "frame", { ...input, positionChanged: false });
  assert.equal(isChatUserScrolling(gesture), false);
  assert.deepEqual(chatScrollGestureAfterEvent(gesture, "frame", input), gesture);
});

test("layout compensation never writes through an owned gesture, including spacer exhaustion", () => {
  for (const base of [20000, 500, 100, 0]) {
    const next = chatSpacerAfterLayout({ base, drift: 0, shift: 300, absorb: true,
      viewportHeight: 800, scrollTop: 3000 });
    assert.equal(next.scrollBy, 0, `base=${base}`);
    assert.equal(next.drift, -300);
    assert.equal(chatSpacerFold(base, next.drift).scrollBy, 300);
  }
});

test("both anchors retain the partially visible tall message instead of the next offscreen message", () => {
  const rows = [
    { id: "tall", key: "tall", ordinal: 0, top: 100 },
    { id: "next", key: "next", ordinal: 0, top: 4100 },
  ];
  assert.equal(captureChatAnchor(rows, 1200)?.id, chatShiftAnchorAt(rows, [4000, 200], 1200)?.id);
  assert.equal(captureChatAnchor(rows, 1200)?.offset, -1100);
});

test("fallback ownership spans the tail rubber-band bounce until stationary frames", () => {
  const input = { supportsScrollEnd: false, atBottom: true, positionChanged: true };
  let gesture = chatScrollGestureAfterEvent(idleChatScrollGesture(), "touchstart", input);
  gesture = chatScrollGestureAfterEvent(gesture, "touchend", input);
  for (const top of [601, 618, 611, 603, 600]) {
    gesture = chatScrollGestureAfterEvent(gesture, "scroll", { ...input, atBottom: top >= 600 });
    assert.equal(isChatUserScrolling(gesture), true);
    gesture = chatScrollGestureAfterEvent(gesture, "frame", input);
  }
  for (let index = 0; index < 3; index++) gesture = chatScrollGestureAfterEvent(gesture, "frame", { ...input, positionChanged: false });
  assert.equal(isChatUserScrolling(gesture), false);
});

test("a top-edge fold clamps once and cannot retain a second correction", () => {
  const fold = chatSpacerFold(0, 286);
  const top = chatScrollWriteTarget({ scrollTop: 117, scrollHeight: 5000, clientHeight: 800 }, 117 + fold.scrollBy, false);
  assert.equal(top, 0);
  assert.equal(fold.drift, 0);
  assert.equal(chatSpacerFold(0, fold.drift).scrollBy, 0);
  assert.equal(chatSpacerHeight(100, -300), 0);
  assert.equal(chatSpacerTranslation(100, -300), -200);
});

test("a translated window selects the same rows as its equivalent layout viewport", () => {
  const translated = chatLayoutViewportTop(500, 20, -100);
  assert.equal(translated, 580);
  const options = { heights: Array.from({ length: 100 }, () => 100), viewportHeight: 300 };
  assert.deepEqual(mountedChatWindow({ ...options, scrollTop: translated }),
    mountedChatWindow({ ...options, scrollTop: chatLayoutViewportTop(600, 20, 0) }));
});
