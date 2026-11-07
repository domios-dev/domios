import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { RuntimeUsageProjection, UsageSnapshot } from "../../types/usage";

async function main() {
  const dom = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
  for (const key of ["window", "document", "navigator", "HTMLElement", "Element", "Node", "Event", "MouseEvent"] as const) Object.defineProperty(globalThis, key, { value: dom[key], configurable: true });
  const bridge = await import("../../lib/bridge");
  const native: RuntimeUsageProjection = { tab_id: "native-tab", thread_id: "native-thread", billing_mode: "subscription", token_usage: null,
    rate_limits: { captured_at: Date.now(), value: [{ window: "primary", usedPercent: 41 }, { window: "secondary", usedPercent: 17 }] } };
  const snapshot = { provider: "fixture-native", captured_at: Date.now() - 1, parsed_ok: true, runtime_usage: [native] } as UsageSnapshot;
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  bridge.__setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    return (command === "usage_latest" ? snapshot : command === "provider_model_usage" ? [] : command === "get_config" ? { ui: { usage_intro_hint_opens: 5 } } : null) as T;
  });
  bridge.__setListenForTest(async () => () => {});
  const { UsageBubble } = await import("./UsageBubble");
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);

  await act(async () => root.render(<UsageBubble provider="fixture-native" tabId="borrowing-tab" threadId="borrowing-thread" borrowedAccount trackingEnabled={false} />));
  assert.equal(container.querySelector("button"), null, "the borrowed account meter follows the native provider's Usage Tracking toggle");
  assert.equal(calls.some(call => call.command === "usage_latest"), false);

  await act(async () => root.render(<UsageBubble key="on" provider="fixture-native" tabId="borrowing-tab" threadId="borrowing-thread" borrowedAccount trackingEnabled />));
  assert.equal(container.querySelector("button")?.getAttribute("aria-label"), "usage session 41% weekly 17%", "the native account's limits, as its own tab shows them");
  assert.ok(calls.some(call => call.command === "usage_latest" && call.args?.provider === "fixture-native"));
  await act(async () => container.querySelector("button")?.dispatchEvent(new dom.MouseEvent("click", { bubbles: true }) as unknown as Event));
  assert.equal(calls.some(call => call.command === "agent_runtime_reset_credits"), false, "a borrowing tab never acts on the native tab's reset credits");
  await act(async () => root.unmount());
  console.log("PASS borrowed account usage: gated by the native toggle, shows the account's reading, no reset actions");
}

void main().catch((error) => { console.error(error); process.exitCode = 1; });
