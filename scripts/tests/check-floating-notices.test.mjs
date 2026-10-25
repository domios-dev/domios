import assert from "node:assert/strict";
import { test } from "node:test";
import { checkFloatingNotices, floatingLiveRegions } from "../check-floating-notices.mjs";

test("rejects fixed live regions and nested announcements", () => {
  assert.deepEqual(floatingLiveRegions('<div className="fixed"><p role="status">Ready</p></div>'), [1]);
  assert.deepEqual(floatingLiveRegions('<div style={{position: "fixed"}} aria-live="polite" />'), [1]);
  assert.deepEqual(floatingLiveRegions('<div className="custom-notice" role="alert" />', "fixture.tsx", new Set(["custom-notice"])), [1]);
});

test("keeps inline feedback and unrelated fixed controls", () => {
  assert.deepEqual(floatingLiveRegions('<><button className="fixed">Back</button><p role="status">Saved</p></>'), []);
});

test("modal feedback stays inline while fixed notices inside it are rejected", () => {
  assert.deepEqual(floatingLiveRegions('<div className="fixed"><div role="dialog" aria-modal="true"><p role="status">No results</p></div></div>'), []);
  assert.deepEqual(floatingLiveRegions('<div role="dialog" aria-modal="true"><p className="fixed" role="status">Saved</p></div>'), [1]);
});

test("the repository has no bespoke fixed live-region notices", () => {
  assert.deepEqual(checkFloatingNotices(), []);
});
