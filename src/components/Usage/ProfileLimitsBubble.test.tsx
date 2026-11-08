import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { UsageSnapshot } from "../../types/usage";

async function main() {
  const dom = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event", "MouseEvent"] as const) Object.defineProperty(globalThis, key, { value: dom[key], configurable: true });
  const bridge = await import("../../lib/bridge");
  let snapshot: UsageSnapshot | null = null;
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  bridge.__setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    return (command === "usage_latest" ? snapshot : null) as T;
  });
  const listeners = new Set<(event: { payload: unknown }) => void>();
  bridge.__setListenForTest(async <T,>(event: string, handler: (event: { payload: T }) => void) => {
    const listener = handler as (event: { payload: unknown }) => void;
    if (event === "usage-snapshot-updated") listeners.add(listener);
    return () => { listeners.delete(listener); };
  });
  const { BalanceBubble, UnavailableLimitsBubble } = await import("./ProfileLimitsBubble");
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const button = () => container.querySelector("button");

  await act(async () => root.render(<UnavailableLimitsBubble reason="Opaque reason" />));
  assert.equal(button()?.getAttribute("aria-label"), "usage limits unavailable: Opaque reason");

  await act(async () => root.render(<BalanceBubble provider="fixture-acp" accountId="profile:fixture" trackingEnabled={false} />));
  assert.equal(button(), null, "a disabled Usage Tracking toggle shows nothing");
  assert.equal(calls.some(call => call.command === "usage_refresh"), false, "and reads nothing");

  await act(async () => root.render(<BalanceBubble key="enabled" provider="fixture-acp" accountId="profile:fixture" trackingEnabled />));
  assert.equal(button()?.getAttribute("aria-label"), "balance not read yet");
  const refreshes = calls.filter(call => call.command === "usage_refresh");
  assert.equal(refreshes.length, 1, "a missing reading is requested once, not polled");
  assert.deepEqual(refreshes[0].args, { provider: "fixture-acp", accountId: "profile:fixture" });
  assert.ok(calls.some(call => call.command === "usage_latest" && call.args?.accountId === "profile:fixture"));

  snapshot = { provider: "fixture-acp", account_id: "profile:fixture", captured_at: Date.now(), parsed_ok: true,
    usage_details: { limits: [], dollar_credits: [], extra_usage: null, balance: { available: true, amounts: [{ currency: "CNY", amount: "110.00" }] } } } as unknown as UsageSnapshot;
  await act(async () => { for (const listener of listeners) listener({ payload: { provider: "fixture-acp", account_id: "profile:fixture", snapshot_id: 1 } }); });
  assert.equal(button()?.getAttribute("aria-label"), "balance 110 CNY");
  assert.equal(calls.filter(call => call.command === "usage_refresh").length, 1);

  snapshot = { provider: "fixture-acp", account_id: "profile:fixture", captured_at: Date.now(), parsed_ok: false, failure_reason: "Opaque failure" } as UsageSnapshot;
  await act(async () => { for (const listener of listeners) listener({ payload: { provider: "fixture-acp", account_id: "profile:fixture", snapshot_id: 2 } }); });
  assert.equal(button()?.getAttribute("aria-label"), "balance unavailable: Opaque failure");

  snapshot = { provider: "fixture-acp", account_id: "profile:fixture", captured_at: Date.now(), parsed_ok: true, failure_reason: "Opaque stale cause",
    usage_details: { limits: [], dollar_credits: [], extra_usage: null, balance: { available: true, amounts: [{ currency: "CNY", amount: "110.00" }] } } } as unknown as UsageSnapshot;
  await act(async () => { for (const listener of listeners) listener({ payload: { provider: "fixture-acp", account_id: "profile:fixture", snapshot_id: 3 } }); });
  assert.equal(button()?.getAttribute("aria-label"), "balance 110 CNY, last read failed: Opaque stale cause");
  await act(async () => root.unmount());

  const failedOnly = createRoot(container);
  snapshot = { provider: "fixture-acp", account_id: "profile:only-failed", captured_at: Date.now(), parsed_ok: false, failure_reason: "Opaque only failure" } as UsageSnapshot;
  calls.length = 0;
  await act(async () => failedOnly.render(<BalanceBubble provider="fixture-acp" accountId="profile:only-failed" trackingEnabled />));
  assert.equal(button()?.getAttribute("aria-label"), "balance unavailable: Opaque only failure", "an account with only failed reads names the failure");
  assert.equal(button()?.textContent, "balance ?");
  await act(async () => { button()?.click(); });
  assert.ok(document.body.textContent?.includes("Opaque only failure"), "the popover carries the reason");
  await act(async () => failedOnly.unmount());

  const unanswered = createRoot(container);
  snapshot = null;
  bridge.__setInvokeForTest(async <T,>(command: string): Promise<T> => {
    if (command === "usage_refresh") throw new Error("Opaque request error");
    return null as T;
  });
  await act(async () => unanswered.render(<BalanceBubble provider="fixture-acp" accountId="profile:unanswered" trackingEnabled />));
  assert.equal(button()?.textContent, "balance ?", "a read request that failed does not keep waiting");
  assert.ok(button()?.getAttribute("aria-label")?.includes("Opaque request error"));
  await act(async () => unanswered.unmount());
  console.log("PASS profile limits bubbles: unavailable reason, gated on-demand balance read and named failure states");
}

void main().catch((error) => { console.error(error); process.exit(1); });
