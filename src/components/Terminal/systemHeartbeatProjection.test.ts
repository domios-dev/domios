import assert from "node:assert/strict";
import test from "node:test";
import { classifyTick, formatCadence, formatTickAt, presentTick, formatTickRow, TICK_REASON_LABELS, type TickFact } from "./systemHeartbeatProjection";

test("log presentation has one outcome and substitutes readable reasons", () => {
  for (const spinalReason of ["same_snapshot", "low_confidence", "no_adapter"]) {
    const tick = { atMs: 0, outcome: "suppressed", spinalVerdict: "suppressed", spinalReason };
    assert.deepEqual(presentTick(tick), { time: formatTickAt(0), outcome: "suppressed", reason: TICK_REASON_LABELS[spinalReason] });
    assert.equal(formatTickRow(tick), `${formatTickAt(0)} · suppressed · ${TICK_REASON_LABELS[spinalReason]}`);
    assert.equal(formatTickRow(tick).split("suppressed").length - 1, 1);
  }
  assert.equal(presentTick({ atMs: 0, outcome: "skipped", spinalReason: "future_reason" }).reason, "future reason");
});

test("tick classification names outcome and spinal handling", () => {
  const cases: Array<[TickFact, string, string, string]> = [
    [{ kind: "accepted", decidedBy: "adapter" }, "sent", "processed", "delivered"],
    [{ kind: "accepted", decidedBy: "cadence" }, "sent", "skipped", "actionable_cadence"],
    [{ kind: "not_due" }, "skipped", "skipped", "not_due"],
    [{ kind: "unchanged_content" }, "skipped", "skipped", "unchanged_content"],
    [{ kind: "unavailable" }, "suppressed", "skipped", "agent_unavailable"],
    [{ kind: "pending" }, "skipped", "processed", "pending"],
    [{ kind: "suppressed", reason: "no_adapter" }, "suppressed", "skipped", "no_adapter"],
    [{ kind: "suppressed", reason: "healthy_snapshot" }, "suppressed", "suppressed", "healthy_snapshot"],
    [{ kind: "suppressed", reason: "model_answer" }, "suppressed", "suppressed", "model_answer"],
    [{ kind: "suppressed", reason: "in_flight" }, "skipped", "skipped", "in_flight"],
    [{ kind: "failed" }, "suppressed", "processed", "delivery_failed"],
  ];
  for (const [fact, outcome, spinalVerdict, spinalReason] of cases) {
    assert.deepEqual(classifyTick(fact), { outcome, spinalVerdict, spinalReason });
  }
});

test("cadence and tick time come from the supplied schedule", () => {
  assert.equal(formatCadence(60_000), "every 1m");
  assert.equal(formatCadence(2_700_000), "every 45m");
  assert.equal(formatCadence(3_600_000), "every 1h");
  assert.equal(formatCadence(1_500), "every 2s");
  assert.equal(formatTickAt(0), "1970-01-01 00:00:00Z");
  assert.equal(formatTickAt(1_700_000_000_000), "2023-11-14 22:13:20Z");
  assert.notEqual(formatTickAt(0), formatTickAt(86_400_000));
});
