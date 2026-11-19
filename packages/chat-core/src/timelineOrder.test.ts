import assert from "node:assert/strict";
import { test } from "node:test";
import { orderTimelineItems } from "./timelineOrder";

test("orders shuffled history pages and host rows by authoritative time", () => {
  const rows = [
    { id: "resume", timestamp: "1770000003000", sourceSessionId: "vm-new", seq: 0 },
    { id: "host-prompt", timestamp: "1770000002000", seq: 1 },
    { id: "history", timestamp: "1770000001000", sourceSessionId: "vm-old", seq: 8 },
  ];
  assert.deepEqual(orderTimelineItems(rows).map(row => row.id), ["history", "host-prompt", "resume"]);
});

test("uses sequence and identity for equal times and preserves source order when provider times are absent", () => {
  const rows = [
    { id: "missing-2", sourceSessionId: "provider", seq: 1 },
    { id: "same-z", timestamp: "1770000001000", sourceSessionId: "provider", seq: 4 },
    { id: "timed", timestamp: "1770000000000", sourceSessionId: "provider", seq: 9 },
    { id: "missing-1", sourceSessionId: "provider", seq: 0 },
    { id: "same-a", timestamp: "1770000001000", sourceSessionId: "provider", seq: 4 },
  ];
  const expected = ["timed", "same-a", "same-z", "missing-2", "missing-1"];
  assert.deepEqual(orderTimelineItems(rows).map(row => row.id), expected);
});

test("interleaves host report cards at their receipt time across resumed VM epochs", () => {
  const rows = [
    { id: "new-answer", timestamp: 300, sourceSessionId: "vm-2", seq: 0 },
    { id: "report-card", timestamp: 200 },
    { id: "old-tool", timestamp: 100, sourceSessionId: "vm-1", seq: 18 },
  ];
  assert.deepEqual(orderTimelineItems(rows).map(row => row.id), ["old-tool", "report-card", "new-answer"]);
});
