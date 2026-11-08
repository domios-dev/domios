import assert from "node:assert/strict";
import type { UsageSnapshot } from "../../types/usage";
import { quotaPill, quotaReadDue, quotaReading, unavailablePill } from "./quotaModel";

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
console.log("quotaModel: unavailable dash with a reason, validated quota shape, closest limit pill and bounded reads");
