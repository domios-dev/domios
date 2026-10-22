import assert from "node:assert/strict";
import { test } from "node:test";
import { sidebarScrollAnchorLayout } from "./sidebarScrollAnchorModel";

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
