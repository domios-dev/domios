import assert from "node:assert/strict";
import { test } from "node:test";
import type { ProductEvent } from "./productAnalytics.generated";
import { emitFeatureUsed, emitProductEvent, isProductEvent, type AnalyticsEndpoint } from "./productAnalytics";

test("UI emitter sends plain counts through the local event endpoint", async () => {
  const calls: Parameters<AnalyticsEndpoint>[] = [];
  const event = { event_type: "workspace_created" as const, event_properties: { workspaces: 3 } };
  await emitProductEvent(async (...args) => { calls.push(args); }, "on", event);
  assert.deepEqual(calls, [["POST", "/api/analytics/event", event]]);
});
test("consent off makes no endpoint request", async () => {
  let calls = 0;
  await emitProductEvent(async () => { calls++; }, "off", { event_type: "analytics_opted_in", event_properties: {} });
  assert.equal(calls, 0);
});
test("analytics delivery failure does not fail the product action", async () => {
  await emitProductEvent(async () => { throw new Error("offline"); }, "on", { event_type: "analytics_opted_in", event_properties: {} });
});
test("UI emitter rejects identifiers, invalid counts and discarded events", async () => {
  for (const event_type of ["tab_opened", "agent_restored", "task_dispatched"]) {
    assert.equal(isProductEvent({ event_type, event_properties: {} }), false);
  }
  assert.equal(isProductEvent({ event_type: "agent_spawned", event_properties: { provider_id: "codex", role: "worker", model: "private" } }), false);
  assert.equal(isProductEvent({ event_type: "agent_spawned", event_properties: { provider_id: "private", role: "worker" } }), false);
  for (const workspaces of [-1, 1.5, "3", NaN, 4294967296]) {
    assert.equal(isProductEvent({ event_type: "workspace_created", event_properties: { workspaces } }), false);
  }
  assert.equal(isProductEvent({ event_type: "app_started", event_properties: { version: "1.2.3-private", platform: "macos", mesh_machines: 1, workspaces: 1 } }), false);
  let calls = 0;
  await emitProductEvent(async () => { calls++; }, "on", { event_type: "workspace_created", event_properties: { workspaces: 1, path: "private" } } as unknown as ProductEvent);
  assert.equal(calls, 0);
});

test("feature use sends only the closed enum after checking consent", async () => {
  for (const consent of ["on", "off"] as const) {
    const calls: unknown[] = [];
    await emitFeatureUsed(async <T>(method: string, path: string, body?: unknown) => {
      calls.push([method, path, body]);
      return { consent } as T;
    }, "web_ui_opened");
    assert.deepEqual(calls, consent === "on" ? [
      ["GET", "/api/analytics/status", undefined],
      ["POST", "/api/analytics/event", { event_type: "feature_used", event_properties: { feature: "web_ui_opened" } }],
    ] : [["GET", "/api/analytics/status", undefined]]);
  }
  assert.equal(isProductEvent({ event_type: "feature_used", event_properties: { feature: "private" } }), false);
});
