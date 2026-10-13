import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { IndexedChatRowGeometry } from "./chatRowGeometry";
import { chatRowOffsets, mountedChatWindowFromGeometry, CHAT_MOUNT_TARGET, CHAT_MOUNT_MAX, type MountedChatWindow } from "./chatMountedWindow";
import { indexChatTranscript, indexedChatAnchor } from "./chatMountedTranscript";

function legacyWindow({ heights, scrollTop, viewportHeight, active = true, pinned = false, targetIndex }: {
  heights: readonly number[];
  scrollTop: number;
  viewportHeight: number;
  active?: boolean;
  pinned?: boolean;
  targetIndex?: number;
}): MountedChatWindow {
  const offsets = chatRowOffsets(heights);
  const count = heights.length;
  const total = offsets[count];
  if (!active || count === 0) return { start: 0, end: 0, top: 0, bottom: total, total };
  const height = Math.max(1, viewportHeight);
  const top = targetIndex === undefined
    ? pinned ? Math.max(0, total - height) : Math.max(0, Math.min(scrollTop, total - height))
    : offsets[Math.max(0, Math.min(count - 1, targetIndex))];
  let visibleStart = 0;
  while (visibleStart < count - 1 && offsets[visibleStart + 1] <= top) visibleStart += 1;
  let visibleEnd = visibleStart + 1;
  while (visibleEnd < count && offsets[visibleEnd] < top + height) visibleEnd += 1;
  let start = visibleStart;
  while (start > 0 && offsets[start] > top - height) start -= 1;
  let end = visibleEnd;
  while (end < count && offsets[end] < top + 2 * height) end += 1;
  const limit = visibleEnd - visibleStart > CHAT_MOUNT_TARGET ? CHAT_MOUNT_MAX : CHAT_MOUNT_TARGET;
  if (end - start > limit) {
    const before = Math.floor(Math.max(0, limit - (visibleEnd - visibleStart)) / 2);
    start = Math.max(start, visibleStart - before);
    end = Math.min(count, start + limit);
  }
  return { start, end, top: offsets[start], bottom: total - offsets[end], total };
}

describe("indexed Chat geometry", () => {
  it("matches the prior window decisions for variable transcripts and viewport inputs", () => {
    let seed = 192;
    const random = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed; };
    for (let sample = 0; sample < 2500; sample++) {
      const heights = Array.from({ length: random() % 500 }, () => random() % 750 + 20);
      const inputs = { scrollTop: random() % 200000 - 1000, viewportHeight: random() % 1400,
        pinned: random() % 3 === 0, active: random() % 5 !== 0,
        targetIndex: random() % 4 === 0 ? random() % 600 - 20 : undefined };
      const geometry = new IndexedChatRowGeometry(heights);
      assert.deepEqual(mountedChatWindowFromGeometry({ geometry, ...inputs }), legacyWindow({ heights, ...inputs }));
    }
  });
  it("updates measured rows and appends across tree boundaries without rebuilding offsets", () => {
    const heights = [100, 450, 200, 0, NaN];
    const geometry = new IndexedChatRowGeometry(heights);
    assert.deepEqual(Array.from({ length: 6 }, (_, index) => geometry.offset(index)), [0, 100, 550, 750, 870, 990]);
    assert.equal(geometry.update(1, 300), true);
    assert.equal(geometry.update(1, 300), false);
    assert.equal(geometry.update(2, NaN), false);
    heights[1] = 300;
    for (let index = 5; index < 40; index++) { heights.push(index * 13); geometry.append(index * 13); }
    assert.deepEqual(Array.from({ length: 41 }, (_, index) => geometry.offset(index)), chatRowOffsets(heights));
  });
  it("reuses geometry for arrivals and resolves stable identities after a prepend", () => {
    const heights = new Map([["a", 90], ["b", 110]]);
    const first = indexChatTranscript([{ id: "a", sourceMessageId: "source" }, { id: "b", sourceMessageId: "source" }], heights);
    first.geometry.update(0, 150);
    const appended = indexChatTranscript([...first.messages, { id: "c" }], heights, first);
    assert.equal(appended.geometry, first.geometry);
    assert.equal(appended.geometry.offset(3), 380);
    const anchor = { key: "source", ordinal: 1, offset: 4 };
    assert.equal(indexedChatAnchor(appended, anchor), 1);
    const prepended = indexChatTranscript([{ id: "older" }, ...appended.messages], heights, appended);
    assert.notEqual(prepended.geometry, appended.geometry);
    assert.equal(indexedChatAnchor(prepended, anchor), 2);
  });
  it("retains every on-screen row even when more than sixty tiny rows fit", () => {
    const geometry = new IndexedChatRowGeometry(Array(500).fill(5));
    const range = mountedChatWindowFromGeometry({ geometry, scrollTop: 800, viewportHeight: 500 });
    assert.ok(range.start <= 160);
    assert.ok(range.end >= 260);
  });
  it("keeps visible rows and overscan on both sides through consecutive range transitions", () => {
    const geometry = new IndexedChatRowGeometry(Array(5000).fill(100));
    for (let top = 10000; top < 11000; top += 17) {
      const range = mountedChatWindowFromGeometry({ geometry, scrollTop: top, viewportHeight: 500 });
      assert.ok(range.start < Math.floor(top / 100));
      assert.ok(range.end > Math.ceil((top + 500) / 100));
    }
  });
});
