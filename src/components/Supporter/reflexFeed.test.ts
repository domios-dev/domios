import assert from "node:assert/strict";
import { test } from "vitest";
import { ACTIVITY_ACTIONS, ACTIVITY_KINDS, ACTIVITY_LABELS, ACTIVITY_REASONS, type SupporterActivity } from "./activity";
import { groupReflexes, reflexOutcome } from "./reflexFeed";

const entry: SupporterActivity = {
  time: 10, team_id: null, kind: "tab_state", label: "ready_idle", action: "notify_only",
  reason: "model_answer", confidence_band: "strong", budget_reached: false, fallback: false, tab_title: "fixture-subject",
};

test("known outcomes render safe subjects and tones", () => {
  for (const [field, values] of Object.entries({ kind: ACTIVITY_KINDS, action: ACTIVITY_ACTIONS, label: ACTIVITY_LABELS, reason: ACTIVITY_REASONS })) {
    for (const value of values) {
      const result = reflexOutcome({ ...entry, [field]: value });
      assert.ok(result.icon);
      assert.ok(result.phrase.length > 0);
      assert.ok(!/redacted|undefined/.test(result.phrase));
      assert.equal(result.subject, entry.tab_title);
    }
  }
  assert.equal(reflexOutcome({ ...entry, fallback: true }).tone, "fallback");
  assert.equal(reflexOutcome({ ...entry, kind: "heartbeat", action: "wake" }).tone, "acted");
  assert.equal(reflexOutcome({ ...entry, kind: "heartbeat", action: "suppress" }).tone, "quiet");
  assert.equal(reflexOutcome({ ...entry, action: "skip" }).tone, "skipped");
});

test("identical consecutive decisions group with their latest finite time", () => {
  const same = groupReflexes([entry, { ...entry, time: 30 }, { ...entry, time: 20 }]);
  assert.equal(same.length, 1);
  assert.equal(same[0].count, 3);
  assert.equal(same[0].time, 30);
  assert.equal(entry.time, 10);
  assert.deepEqual(groupReflexes([]), []);
  assert.equal(groupReflexes([{ ...entry, time: NaN }, entry])[0].time, entry.time);
});

test("changed decisions interrupt grouping for their own tab", () => {
  for (const changed of [
    { kind: "heartbeat" }, { action: "wake" }, { label: "working_normally" },
    { fallback: true }, { reason: "startup_deadline" }, { confidence_band: "weak" },
  ]) {
    const rows = groupReflexes([entry, { ...entry, ...changed }, entry]);
    assert.equal(rows.length, 3);
    assert.deepEqual(rows.map(row => row.count), [1, 1, 1]);
  }
});

test("interleaved tabs preserve consecutive grouping within each tab", () => {
  const rows = groupReflexes([
    { ...entry, tab_id: "a" }, { ...entry, tab_id: "b" }, { ...entry, tab_id: "a", time: 20 },
  ]);
  assert.equal(rows.length, 2);
  assert.deepEqual(rows.map(row => row.count), [2, 1]);
});

test("tabs with the same title stay distinct", () => {
  assert.equal(groupReflexes([{ ...entry, tab_id: "a" }, { ...entry, tab_id: "b" }]).length, 2);
});

test("private question prose and unknown fields never supply display text", () => {
  const absent = reflexOutcome({ ...entry, tab_title: undefined, asked: [{ name: "fixture", question: "private-question", answer: "private-answer" }] });
  assert.equal(absent.subject, undefined);
  assert.ok(!absent.phrase.includes("private-"));
  const unsafe = "private-opaque-value";
  const unknown = reflexOutcome({ ...entry, tab_title: null, kind: unsafe, label: unsafe, action: unsafe, reason: unsafe });
  assert.ok(unknown.icon);
  assert.ok(!unknown.phrase.includes(unsafe));
});
