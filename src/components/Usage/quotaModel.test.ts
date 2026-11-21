import assert from "node:assert/strict";
import type { UsageSnapshot } from "../../types/usage";
import {
  activeQuotaReset,
  formatResetCountdown,
  nextQuotaResetTickMs,
  quotaPill,
  quotaReadDue,
  quotaReading,
  quotaResetPill,
  unavailablePill,
} from "./quotaModel";

const unavailable = unavailablePill("Opaque reason");
assert.equal(unavailable.text, "—", "nothing to show is a quiet dash");
assert.ok(unavailable.label.includes("Opaque reason"), "the accessible name still says why");

const row = (details: unknown, extra: Partial<UsageSnapshot> = {}) => ({provider: "fixture", account_id: "profile:fixture", captured_at: 1_000, parsed_ok: true, usage_details: details, ...extra}) as UsageSnapshot;
const limits = [{label: "fixture-pro", used_percent: 12.5, resets_at: 5_000}, {label: "fixture-lite", used_percent: 3, resets_at: null}];
const reading = quotaReading(row({quota: {title: "Fixture quota", limits}}));
assert.deepEqual(reading, {title: "Fixture quota", limits: [{label: "fixture-pro", usedPercent: 12.5, resetsAt: 5_000}, {label: "fixture-lite", usedPercent: 3, resetsAt: null}]});
for (const details of [null, {}, {quota: {title: "Fixture quota", limits: []}}, {quota: {limits}}, {quota: {title: "Fixture quota", limits: [{label: "fixture-pro"}]}}, {quota: {title: "Fixture quota", limits: [{label: "fixture-pro", used_percent: "12", resets_at: null}]}}, {quota: {title: "Fixture quota", limits: [{label: "fixture-pro", used_percent: 101, resets_at: null}]}}]) {
  assert.equal(quotaReading(row(details)), null, JSON.stringify(details));
}
assert.equal(quotaReading(row({quota: {title: "Fixture quota", limits}}, {parsed_ok: false})), null, "failure rows carry no quota");

const pill = quotaPill(reading!, null);
assert.equal(pill.text, "13%", "the pill shows the model closest to its limit");
assert.equal(pill.percent, 12.5);
assert.ok(pill.label.includes("fixture-pro"), "the accessible name says which model that is");
assert.ok(quotaPill(reading!, "Opaque stale cause").label.includes("Opaque stale cause"), "a kept reading names what stopped it moving");

assert.equal(quotaReadDue(null, 0), true, "the first read is always due");
assert.equal(quotaReadDue(1_000, 1_000 + 59_999), false, "turn activity does not read faster than once a minute");
assert.equal(quotaReadDue(1_000, 1_000 + 60_000), true);

const RESET_NOW = 1_795_280_000_000;
const activeReset = activeQuotaReset(
  row({ quota_reset: { resets_at: RESET_NOW + 51 * 60_000, observed_at: RESET_NOW - 1000, reason: "Quota reached" } }),
  RESET_NOW,
);
assert.deepEqual(activeReset, { resetsAt: RESET_NOW + 51 * 60_000, observedAt: RESET_NOW - 1000, reason: "Quota reached" });
assert.deepEqual(quotaResetPill(activeReset!, RESET_NOW), {
  text: "Quota · resets in 51m",
  label: "quota limit reached, resets in 51m",
});
assert.equal(formatResetCountdown(RESET_NOW + 2 * 3_600_000 + 15 * 60_000, RESET_NOW), "2h 15m");
assert.equal(formatResetCountdown(RESET_NOW + 30_000, RESET_NOW), "<1m");
assert.equal(activeQuotaReset(row({ quota_reset: { resets_at: RESET_NOW } }), RESET_NOW), null, "expired quota reset clears automatically");
assert.equal(activeQuotaReset(row({ quota_reset: { resets_at: "bad" } }), RESET_NOW), null, "malformed quota reset fails closed");
assert.equal(nextQuotaResetTickMs(RESET_NOW + 20_000, RESET_NOW), 20_050);
assert.equal(nextQuotaResetTickMs(RESET_NOW, RESET_NOW), null);
console.log("quotaModel: unavailable dash with a reason, validated quota shape, closest limit pill, bounded reads and active quota-reset expiry");
