import assert from "node:assert/strict";
import { unreportedBillingLabel, usageMetricsAriaLabel, visibleUsageMetrics } from "./metrics";

const weeklyOnly = visibleUsageMetrics({
  session_pct: null,
  session_resets_at: null,
  weekly_pct: 5,
  weekly_resets_at: 123,
});
assert.deepEqual(weeklyOnly, [{ key: "weekly", label: "Weekly", pct: 5, resetsAt: 123 }]);
assert.equal(usageMetricsAriaLabel(weeklyOnly), "usage weekly 5%");

const both = visibleUsageMetrics({
  session_pct: 20,
  session_resets_at: 100,
  weekly_pct: 40,
  weekly_resets_at: 200,
});
assert.deepEqual(both.map((metric) => metric.key), ["session", "weekly"]);
assert.equal(usageMetricsAriaLabel(both), "usage session 20% weekly 40%");

console.log("usage metrics: all passed");

const subscription = { billing_mode: "subscription", limits_reported: false, session_pct: null, weekly_pct: null, spend_cents: null } as const;
assert.equal(unreportedBillingLabel(subscription), "subscription · limits not reported");
assert.equal(unreportedBillingLabel({ ...subscription, weekly_pct: 0 }), null);
assert.equal(unreportedBillingLabel({ ...subscription, limits_reported: true }), null);
assert.equal(unreportedBillingLabel({ ...subscription, billing_mode: "api" }), null);
assert.equal(unreportedBillingLabel({ ...subscription, billing_mode: "unknown" }), null);
