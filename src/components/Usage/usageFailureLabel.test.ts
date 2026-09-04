import assert from "node:assert/strict";
import test from "node:test";
import { usageFailureLabel } from "./UsageTable";
import type { UsageSnapshot } from "../../types/usage";

const row = (failure_reason: string | null): UsageSnapshot =>
  ({ failure_reason, parsed_ok: false } as unknown as UsageSnapshot);

// The pill used to read "timeout" for every failed probe, so a parse or fetch
// error was reported as a probe that never answered. The kind is on the row.
test("each failure kind gets its own label, and none of them borrows timeout", () => {
  const kinds = [
    "TIMEOUT",
    "FETCH_ERROR",
    "SOURCE_UNAVAILABLE",
    "INTERNAL",
    "PARSE_ERROR",
    "SKIP",
    "NO_PARSER",
    "DB_INSERT",
  ];
  const labels = kinds.map((kind) => usageFailureLabel(row(`${kind}: detail`)));
  assert.equal(new Set(labels).size, kinds.length, `labels must be distinct: ${labels.join(", ")}`);
  assert.equal(labels.filter((l) => l === usageFailureLabel(row("TIMEOUT: x"))).length, 1);
});

test("a non-timeout kind never renders as timeout", () => {
  const timeout = usageFailureLabel(row("TIMEOUT: probe did not answer"));
  for (const kind of ["FETCH_ERROR", "PARSE_ERROR", "SOURCE_UNAVAILABLE"]) {
    assert.notEqual(usageFailureLabel(row(`${kind}: detail`)), timeout);
  }
});

test("an absent or unrecognised reason is neither guessed nor blank", () => {
  assert.equal(usageFailureLabel(row(null)), "failed");
  assert.equal(usageFailureLabel(row("SOMETHING_NEW: detail")), "failed");
});
