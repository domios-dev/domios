// Run: npx tsx src/components/Canvas/missionControl/planSettingsModel.test.ts

import assert from "node:assert/strict";
import { HEARTBEAT_INTERVALS, intervalLabel, oversightLines, oversightSummary, pauseHint, type PlanOversight } from "./planSettingsModel";

const M = 60_000;
const NOW = 1_700_000_000_000;
const base: PlanOversight = {
  mode: "curator", heartbeat_interval_ms: 30 * M, next_at: NOW + 62 * M, last_round_at: NOW - 38 * M,
  fallback_reason: null, paused: false, paused_at: null, pause_reason: null,
};

assert.deepEqual(HEARTBEAT_INTERVALS.map(option => option.ms), [15, 30, 45, 60, 90, 120].map(minutes => minutes * M));
assert.equal(HEARTBEAT_INTERVALS.find(option => option.label === "60m")?.ms, 60 * M);
assert.equal(intervalLabel(30 * M), "30m");
assert.equal(intervalLabel(2 * 60 * M), "120m");
assert.equal(intervalLabel(null), "machine default");
for (const reason of ["reminders_disabled", "waiting_on_human", "complete"]) {
  assert.equal(oversightLines({ ...base, suspended_reason: reason }, NOW).length, 1);
  assert.equal(oversightLines({ ...base, suspended_reason: reason }, NOW)[0].tone, "muted");
}

assert.deepEqual(oversightLines(base, NOW), [{ tone: "accent", text: "Next round in ~1h, last round 38m ago" }]);
assert.deepEqual(oversightLines({ ...base, last_round_at: null }, NOW), [{ tone: "accent", text: "Next round in ~1h" }]);
assert.deepEqual(oversightLines({ ...base, mode: "heartbeat", next_at: NOW + 18 * M }, NOW), [{ tone: "accent", text: "Next reminder in 18m" }]);
assert.deepEqual(oversightLines({ ...base, fallback_reason: "opaque reason", next_at: NOW + 18 * M }, NOW), [
  { tone: "warning", text: "Curator unavailable, heartbeat running: opaque reason" },
  { tone: "muted", text: "Next reminder in 18m" },
]);
assert.equal(oversightLines({ ...base, paused: true }, NOW)[0].tone, "muted");
assert.equal(oversightLines({ ...base, next_at: null }, NOW)[0].text.startsWith("Next round not scheduled"), true);

assert.equal(oversightSummary(base, NOW), "Curator, next round in ~1h");
assert.deepEqual(oversightLines({ ...base, next_at: null, running_round: 81 }, NOW), [{ tone: "green", text: "Round 81 running" }]);
assert.equal(oversightSummary({ ...base, next_at: null, running_round: 81 }, NOW), "Curator, round 81 running");
assert.equal(oversightSummary({ ...base, mode: "heartbeat", next_at: NOW + 18 * M }, NOW), "Heartbeat every 30m, next in 18m");
assert.equal(oversightSummary({ ...base, fallback_reason: "x", next_at: NOW + 18 * M }, NOW), "Heartbeat every 30m, next in 18m");
assert.equal(oversightSummary({ ...base, paused: true }, NOW), "Paused");

assert.equal(pauseHint(base, NOW, "running hint"), "running hint");
assert.equal(pauseHint({ ...base, paused: true, paused_at: NOW - 25 * M, pause_reason: "opaque" }, NOW, "running hint"),
  "Paused 25m ago: opaque. No heartbeats or dispatch until resumed.");
assert.equal(pauseHint({ ...base, paused: true }, NOW, "running hint"), "Paused. No heartbeats or dispatch until resumed.");

console.log("plan settings model assertions passed");
