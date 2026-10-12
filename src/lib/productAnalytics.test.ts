import assert from "node:assert/strict";
import { test } from "node:test";
import { emitProductEvent, isProductEvent, type AnalyticsEndpoint } from "./productAnalytics";

test("UI emitter uses the local event endpoint with the closed catalog body", async () => {
  const calls: Parameters<AnalyticsEndpoint>[] = [];
  const endpoint: AnalyticsEndpoint = async (...args) => { calls.push(args); };
  await emitProductEvent(endpoint, "on", { event_type: "analytics_opted_in", event_properties: {} });
  assert.deepEqual(calls, [["POST", "/api/analytics/event", { event_type: "analytics_opted_in", event_properties: {} }]]);
});
test("consent off makes no endpoint request", async () => {
  let calls = 0;
  await emitProductEvent(async () => { calls++; }, "off", { event_type: "analytics_opted_in", event_properties: {} });
  assert.equal(calls, 0);
});
test("analytics delivery failure does not fail the product action", async () => {
  await emitProductEvent(async () => { throw new Error("offline"); }, "on", { event_type: "analytics_opted_in", event_properties: {} });
});

test("UI emitter refuses unknown properties and enum values before sending", async () => {
  let calls = 0;
  const endpoint: AnalyticsEndpoint = async () => { calls++; };
  const extra = { event_type: "analytics_opted_in" as const, event_properties: { path: "private" } };
  const extraTab = { event_type: "tab_opened" as const, event_properties: { kind: "shell" as const, provider_id: null, path: "private" } };
  await emitProductEvent(endpoint, "on", extraTab);
  assert.equal(calls, 0);
  assert.equal(isProductEvent(extra), false);
  assert.equal(isProductEvent({ event_type: "agent_spawned", event_properties: { provider_id: "private", role: "worker", model_family: "unknown" } }), false);
  assert.equal(isProductEvent({ event_type: "app_started", event_properties: { version: "1.2.3-private", platform: "macos", update_channel: "stable" } }), false);
  assert.equal(isProductEvent({ event_type: "update_installed", event_properties: { from_version: "1.2.3", to_version: "1.2.4", kind: "frontend", attempt_count: -1 } }), false);
});

test("UI dispatch emitter preserves unspecified intent without guessing", async () => {
  const calls: Parameters<AnalyticsEndpoint>[] = [];
  await emitProductEvent(async (...args) => { calls.push(args); }, "on", {
    event_type: "task_dispatched", event_properties: { execution_intent: "unspecified" },
  });
  assert.deepEqual(calls, [["POST", "/api/analytics/event", {
    event_type: "task_dispatched", event_properties: { execution_intent: "unspecified" },
  }]]);
});
