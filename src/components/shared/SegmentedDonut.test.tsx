// Run: npx tsx src/components/shared/SegmentedDonut.test.tsx

import assert from "node:assert/strict";
import {
  segmentedDonutBackground,
  type SegmentedDonutSegment,
} from "./SegmentedDonut";

const segments = (
  todo: number,
  review: number,
  blocked: number,
  done: number,
): SegmentedDonutSegment[] => [
  { status: "todo", count: todo },
  { status: "review", count: review },
  { status: "blocked", count: blocked },
  { status: "done", count: done },
];

async function main() {
  assert.equal(
    segmentedDonutBackground(segments(2, 1, 1, 4)),
    "conic-gradient(var(--ct-accent) 0% 25%, var(--ct-yellow) 25% 37.5%, var(--ct-red) 37.5% 50%, var(--ct-green) 50% 100%)",
    "mixed segments are contiguous and preserve status order",
  );
  assert.equal(
    segmentedDonutBackground(segments(0, 0, 3, 0)),
    "conic-gradient(var(--ct-red) 0% 100%)",
    "blocked-only input fills the ring",
  );
  assert.equal(
    segmentedDonutBackground(segments(0, 0, 0, 5)),
    "conic-gradient(var(--ct-green) 0% 100%)",
    "all-done input fills the ring",
  );
  assert.equal(
    segmentedDonutBackground(segments(0, 0, 0, 0)),
    "color-mix(in srgb, var(--ct-fg) 14%, transparent)",
    "empty input uses a theme-derived neutral ring",
  );
  assert.equal(
    segmentedDonutBackground([{ count: 1, color: "var(--ct-purple)" }]),
    "conic-gradient(var(--ct-purple) 0% 100%)",
    "callers may provide an explicit runtime-theme colour",
  );

  console.log("SegmentedDonut tests passed");
}

void main();
