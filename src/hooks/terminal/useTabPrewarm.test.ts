import assert from "node:assert/strict";
import test from "node:test";
import {
  recordRecentTab,
  scheduleSequentialPrewarm,
  selectPrewarmTabIds,
  TAB_PREWARM_LIMIT,
} from "./useTabPrewarm";

test("recent tabs prewarm before adjacent tabs", () => {
  const recent = recordRecentTab(["tab-a", "tab-d"], "tab-c");
  assert.deepEqual(recent, ["tab-c", "tab-a", "tab-d"]);
  assert.deepEqual(selectPrewarmTabIds(["tab-a", "tab-b", "tab-c", "tab-d"], "tab-c", recent), [
    "tab-a",
    "tab-d",
  ]);
});

test("adjacent tabs fill an empty recent set in nearest-first order", () => {
  assert.deepEqual(selectPrewarmTabIds(["tab-a", "tab-b", "tab-c", "tab-d"], "tab-c", []), [
    "tab-b",
    "tab-d",
  ]);
});

test("prewarm stays bounded and ignores tabs outside the active workspace", () => {
  const selected = selectPrewarmTabIds(
    ["tab-a", "tab-b", "tab-c", "tab-d"],
    "tab-b",
    ["foreign", "tab-d", "tab-c", "tab-a"],
  );
  assert.equal(selected.length, TAB_PREWARM_LIMIT);
  assert.deepEqual(selected, ["tab-d", "tab-c"]);
});

test("recent history remains bounded to active plus prewarm capacity", () => {
  assert.deepEqual(recordRecentTab(["tab-b", "tab-c", "tab-d", "tab-e"], "tab-a"), [
    "tab-a",
    "tab-b",
    "tab-c",
  ]);
});

test("prewarm mounts one tab per frame and cancellation stops the sequence", () => {
  const frames: Array<() => void> = [];
  const cancelled: number[] = [];
  const mounted: string[] = [];
  let nextHandle = 0;
  const scheduler = {
    request(callback: () => void) {
      frames.push(callback);
      nextHandle += 1;
      return nextHandle;
    },
    cancel(handle: number) {
      cancelled.push(handle);
    },
  };

  const cancel = scheduleSequentialPrewarm(["tab-a", "tab-b"], (id) => mounted.push(id), scheduler);
  assert.deepEqual(mounted, []);
  frames.shift()?.();
  assert.deepEqual(mounted, ["tab-a"]);
  cancel();
  frames.shift()?.();
  assert.deepEqual(mounted, ["tab-a"]);
  assert.deepEqual(cancelled, [2]);
});
