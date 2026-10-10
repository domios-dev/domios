import assert from "node:assert/strict";
import { ACTIVITY_ACTIONS, ACTIVITY_KINDS, ACTIVITY_LABELS, ACTIVITY_REASONS, type SupporterActivity } from "./activity";
import { groupReflexes, reflexOutcome } from "./reflexFeed";

const entry: SupporterActivity = {
  time: 10, team_id: null, kind: "tab_state", label: "ready_idle", action: "notify_only",
  reason: "model_answer", confidence_band: "strong", budget_reached: false, fallback: false, tab_title: "fixture-subject",
};
for (const [field, values] of Object.entries({ kind: ACTIVITY_KINDS, action: ACTIVITY_ACTIONS, label: ACTIVITY_LABELS, reason: ACTIVITY_REASONS })) {
  for (const value of values) {
    const result = reflexOutcome({ ...entry, [field]: value });
    assert.ok(result.icon, `${field}:${value} icon`);
    assert.ok(result.phrase.length > 0, `${field}:${value} phrase`);
    assert.ok(!/redacted|undefined/.test(result.phrase), `${field}:${value} safe outcome`);
    assert.equal(result.subject, entry.tab_title);
  }
}
for (const kind of ACTIVITY_KINDS) {
  for (const action of ACTIVITY_ACTIONS) {
    const result = reflexOutcome({ ...entry, kind, action });
    assert.ok(result.icon);
    assert.ok(result.phrase);
  }
}
const same = groupReflexes([entry, { ...entry, time: 30 }, { ...entry, time: 20 }]);
assert.equal(same.length, 1);
assert.equal(same[0].count, 3);
assert.equal(same[0].time, 30);
assert.equal(entry.time, 10);
assert.deepEqual(groupReflexes([]), []);
for (const changed of [
  { kind: "heartbeat" }, { action: "wake" }, { label: "working_normally" },
  { tab_title: "another-subject" }, { fallback: true },
]) {
  const different = { ...entry, ...changed };
  const rows = groupReflexes([entry, different, entry]);
  assert.equal(rows.length, 3);
  assert.deepEqual(rows.map((row) => row.count), [1, 1, 1]);
}
const laterOutcome = { ...entry, time: 30, reason: "startup_deadline" };
const changedReason = groupReflexes([entry, laterOutcome]);
assert.equal(changedReason.length, 1);
assert.equal(changedReason[0].count, 2);
assert.equal(changedReason[0].phrase, reflexOutcome(laterOutcome).phrase);
assert.equal(reflexOutcome({ ...entry, fallback: true }).tone, "fallback");
assert.equal(reflexOutcome({ ...entry, kind: "heartbeat", action: "wake" }).tone, "acted");
assert.equal(reflexOutcome({ ...entry, kind: "heartbeat", action: "suppress" }).tone, "quiet");
assert.equal(reflexOutcome({ ...entry, action: "skip" }).tone, "skipped");
assert.equal(reflexOutcome({ ...entry, reason: "route_failed", action: "send" }).tone, "skipped");
assert.equal(reflexOutcome({ ...entry, reason: "provider_safe_step_stale", action: "recovery" }).tone, "skipped");
const absent = reflexOutcome({ ...entry, tab_title: undefined, asked: [{ name: "fixture", question: "private-question", answer: "private-answer" }] });
assert.equal(absent.subject, undefined);
assert.ok(!absent.phrase.includes("private-"));
const unsafe = "private-opaque-value";
const unknown = reflexOutcome({ ...entry, tab_title: null, kind: unsafe, label: unsafe, action: unsafe, reason: unsafe });
assert.ok(unknown.icon);
assert.ok(unknown.phrase);
assert.ok(!unknown.phrase.includes(unsafe));
assert.equal(groupReflexes([{ ...entry, time: NaN }, entry])[0].time, entry.time);
console.log("reflexFeed: allowlist coverage, grouping, subjects and tones passed");
