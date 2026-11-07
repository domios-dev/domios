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
  const now = Date.now();
  const native: RuntimeUsageProjection = { tab_id: "native-tab", thread_id: "native-thread", billing_mode: "subscription", token_usage: null,
    rate_limits: { captured_at: now, value: [{ window: "primary", usedPercent: 41 }, { window: "secondary", usedPercent: 17 }] },
    account_credits: { kind: "balance", balance: "15765.14" },
    resets: { availableCount: 2, nextExpiry: null, credits: [
      { id: "first", status: "available", expiresAt: now + 86_400_000 * 9, title: "Full reset" },
      { id: "second", status: "available", expiresAt: now + 86_400_000 * 9, title: "Full reset" },
    ] } };
  const snapshot = { provider: "fixture-native", captured_at: now - 1, parsed_ok: true, runtime_usage: [native] } as UsageSnapshot;
  const projection = { session: { projected_pct: 78, rate_per_min: 0.1, will_exceed: false }, weekly: null };
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  bridge.__setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    const answers: Record<string, unknown> = { usage_latest: snapshot, usage_projection: projection, usage_history: [], provider_model_usage: [], get_config: { ui: { usage_intro_hint_opens: 5 } } };
    return (answers[command] ?? null) as T;
  });
  bridge.__setListenForTest(async () => () => {});
  const { UsageBubble } = await import("./UsageBubble");
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);

  await act(async () => root.render(<UsageBubble provider="fixture-native" tabId="borrowing-tab" threadId="borrowing-thread" borrowedAccount trackingEnabled={false} />));
  assert.equal(container.querySelector("button"), null, "the borrowed account meter follows the native provider's Usage Tracking toggle");
  assert.equal(calls.some(call => call.command === "usage_latest"), false);

  /** Opens one tab's popover and returns its account rows, with the tab's own reset actions set apart. */
  async function accountRows(tab: { tabId: string; threadId: string; borrowedAccount?: boolean }) {
    await act(async () => root.render(<UsageBubble key={tab.tabId} provider="fixture-native" trackingEnabled {...tab} />));
    const label = container.querySelector("button")?.getAttribute("aria-label");
    await act(async () => container.querySelector("button")?.dispatchEvent(new dom.MouseEvent("click", { bubbles: true }) as unknown as Event));
    const dialog = document.querySelector('[role="dialog"][aria-label="usage details"]');
    assert.ok(dialog, `${tab.tabId} opens its usage details`);
    const resets = dialog.querySelector('section[aria-label="Session resets"]');
    const actions = [...dialog.querySelectorAll("button")].filter(button => button.getAttribute("aria-label")?.startsWith("Use ")).map(button => button.getAttribute("aria-label"));
    const resetRows = [...resets?.querySelectorAll("li") ?? []].map(row => [...row.childNodes].filter(node => node.nodeName !== "BUTTON").map(node => node.textContent).join(""));
    const rest = dialog.cloneNode(true) as Element;
    rest.querySelector('section[aria-label="Session resets"]')?.remove();
    const rows = { label, text: rest.textContent, resetHeading: resets?.firstElementChild?.textContent, resetRows };
    await act(async () => root.render(<></>));
    return { rows, actions };
  }

  const own = await accountRows({ tabId: "native-tab", threadId: "native-thread" });
  const borrowed = await accountRows({ tabId: "borrowing-tab", threadId: "borrowing-thread", borrowedAccount: true });
  assert.equal(own.rows.label, "usage session 41% weekly 17%");
  assert.match(own.rows.text ?? "", /Credits15,765\.14/);
  assert.match(own.rows.text ?? "", /78%/, "the session projection marker");
  assert.equal(own.rows.resetHeading, "Session resets2");
  assert.equal(own.rows.resetRows.length, 2);
  assert.deepEqual(borrowed.rows, own.rows, "a borrowing tab renders the native tab's account rows for the same snapshot");
  assert.deepEqual(own.actions, ["Use Full reset 1", "Use Full reset 2"]);
  assert.deepEqual(borrowed.actions, [], "a borrowing tab never offers the native tab's reset actions");
  assert.ok(calls.some(call => call.command === "usage_latest" && call.args?.provider === "fixture-native"));
  assert.equal(calls.some(call => call.command === "agent_runtime_reset_credits" && call.args?.tabId === "borrowing-tab"), false, "a borrowing tab never acts on the native tab's reset credits");
  await act(async () => root.unmount());
  console.log("PASS borrowed account usage: gated by the native toggle, the native tab's account rows, no reset actions");
}

void main().catch((error) => { console.error(error); process.exitCode = 1; });
