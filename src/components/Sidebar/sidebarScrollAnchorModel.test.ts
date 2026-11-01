import assert from "node:assert/strict";
import { test } from "node:test";
import { sidebarAnchorShift, sidebarScrollAnchorLayout, sidebarTrailingSpaceAfterScroll } from "./sidebarScrollAnchorModel";

test("collapse at maximum retains scroll offset with only the missing scroll range", () => {
  assert.deepEqual(sidebarScrollAnchorLayout(427, 1011, 716), { scrollTop: 427, trailingSpace: 132 });
});

test("scrolling up releases trailing space progressively and completely at the natural end", () => {
  assert.deepEqual(sidebarScrollAnchorLayout(377, 1011, 716), { scrollTop: 377, trailingSpace: 82 });
  assert.deepEqual(sidebarScrollAnchorLayout(295, 1011, 716), { scrollTop: 295, trailingSpace: 0 });
});

test("expansion consumes the temporary space without changing the retained offset", () => {
  assert.deepEqual(sidebarScrollAnchorLayout(427, 1143, 716), { scrollTop: 427, trailingSpace: 0 });
});

test("non-clamped and fractional offsets need no unnecessary space", () => {
  assert.deepEqual(sidebarScrollAnchorLayout(250, 1143, 716), { scrollTop: 250, trailingSpace: 0 });
  assert.deepEqual(sidebarScrollAnchorLayout(295.25, 1011, 716), { scrollTop: 295.25, trailingSpace: 1 });
  assert.deepEqual(sidebarScrollAnchorLayout(-50, 200, 716), { scrollTop: 0, trailingSpace: 0 });
});

test("content that fits the viewport keeps no offset, because a retained one would be unreachable", () => {
  assert.deepEqual(sidebarScrollAnchorLayout(100, 716, 716), { scrollTop: 0, trailingSpace: 0 });
  assert.deepEqual(sidebarScrollAnchorLayout(34, 600, 716), { scrollTop: 0, trailingSpace: 0 });
  assert.deepEqual(sidebarScrollAnchorLayout(100, 717, 716), { scrollTop: 100, trailingSpace: 99 });
});

test("a user scroll only releases trailing space and never grows it", () => {
  assert.equal(sidebarTrailingSpaceAfterScroll(377, 1011, 716, 132), 82);
  assert.equal(sidebarTrailingSpaceAfterScroll(295, 1011, 716, 132), 0);
  assert.equal(sidebarTrailingSpaceAfterScroll(600, 1011, 716, 40), 40);
  assert.equal(sidebarTrailingSpaceAfterScroll(100, 1011, 716, 0), 0);
});

test("sub-pixel anchor drift is not a scroll correction; real shifts are", () => {
  assert.equal(sidebarAnchorShift(120.3, 120), 0);
  assert.equal(sidebarAnchorShift(119.6, 120), 0);
  assert.equal(sidebarAnchorShift(180, 120), 60);
  assert.equal(sidebarAnchorShift(100, 120), -20);
});
