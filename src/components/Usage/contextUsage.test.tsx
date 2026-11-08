import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { UsageSnapshot } from "../../types/usage";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { ComposerContextMeter } from "../Terminal/ComposerContextMeter";
import { UnavailableLimitsBubble } from "./ProfileLimitsBubble";
import { UsageBubble } from "./UsageBubble";
import { contextUsage, hasQuota } from "./contextUsage";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("../../lib/bridge", async (original) => ({ ...(await original<typeof import("../../lib/bridge")>()), invoke: bridge.invoke, listen: bridge.listen }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const fill = { used_tokens: 84_000, window_tokens: 200_000 };

describe("the context reading behind the usage bubble", () => {
  it("is the reported used/window fill, with tokens and cost only when reported", () => {
    expect(contextUsage(undefined, null)).toBeNull();
    expect(contextUsage({ used_tokens: 10 }, null)).toBeNull();
    const plain = contextUsage(fill, null)!;
    expect(plain.percent).toBe(42);
    expect(plain.sessionTokens).toBeNull();
    expect(plain.costMicros).toBeNull();
    expect(contextUsage({ ...fill, cost_usd: 0.42 }, null)!.costMicros).toBe(420_000);
    const reading = { tab_id: "acp", thread_id: "t", billing_mode: "unknown" as const, rate_limits: null,
      token_usage: { captured_at: 1, value: { input_tokens: 50_000, output_tokens: 2_340, cost_micros: 1_250_000 } } };
    expect(contextUsage(fill, reading)).toMatchObject({ sessionTokens: 52_340, costMicros: 1_250_000 });
    expect(hasQuota(null)).toBe(false);
    expect(hasQuota({ session_pct: 62 } as UsageSnapshot)).toBe(true);
  });
});

let root: Root | null = null;
let host: HTMLDivElement;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  bridge.invoke.mockReset();
  bridge.listen.mockReset();
});

async function mount(context: unknown, snapshot: UsageSnapshot | null = null) {
  bridge.listen.mockResolvedValue(() => undefined);
  bridge.invoke.mockImplementation(async (command: string) => command === "usage_latest" ? snapshot : command === "provider_model_usage" ? [] : null);
  const store = createMuxStateStore(() => ({ tabs: { acp: { id: "acp", context } } } as never));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<MuxStateStoreContext.Provider value={store}>
    <UsageBubble provider="acp" providerLabel="Gemini CLI" tabId="acp" threadId="t" trackingEnabled context={context} />
    <ComposerContextMeter tabId="acp" />
  </MuxStateStoreContext.Provider>));
  await act(async () => { await Promise.resolve(); });
}

describe("the usage bubble for a harness without quota", () => {
  it("shows the reported context fill and cost, and its popover holds the gauge and cost", async () => {
    await mount({ ...fill, cost_usd: 0.42 });
    const bubble = host.querySelector<HTMLButtonElement>('[data-testid="usage-context-bubble"] button')!;
    expect(bubble.textContent).toBe("ctx 42%");
    expect(bubble.getAttribute("aria-label")).toBe("context 42% of window · $0.42");
    // The separate context meter does not repeat the fill the bubble shows.
    expect(host.querySelector('meter[aria-label="Context window"]')).toBeNull();
    await act(async () => bubble.click());
    const popover = document.body.querySelector('[role="dialog"][aria-label="usage details"]')!;
    expect(popover.textContent).toContain("Gemini CLI usage");
    expect(popover.textContent).toContain("Context window");
    expect(popover.textContent).toContain("84K of 200K tokens");
    expect(popover.textContent).toContain("Session cost$0.42");
    expect(popover.textContent).not.toContain("Session tokens");
  });

  it("replaces an unavailable source's dash with the reported fill, once, and keeps the reason", async () => {
    bridge.listen.mockResolvedValue(() => undefined);
    bridge.invoke.mockImplementation(async () => null);
    const store = createMuxStateStore(() => ({ tabs: { acp: { id: "acp", context: fill } } } as never));
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root!.render(<MuxStateStoreContext.Provider value={store}>
      <UnavailableLimitsBubble reason="Opaque reason" session={{ provider: "acp", tabId: "acp", threadId: "t", context: fill, label: "Fixture" }} />
      <ComposerContextMeter tabId="acp" />
    </MuxStateStoreContext.Provider>));
    const bubble = host.querySelector<HTMLButtonElement>('[data-testid="usage-context-bubble"] button')!;
    expect(bubble.getAttribute("aria-label")).toBe("context 42% of window");
    expect(host.querySelector('meter[aria-label="Context window"]')).toBeNull();
    await act(async () => bubble.click());
    expect(document.body.querySelector('[role="dialog"][aria-label="usage details"]')!.textContent).toContain("Opaque reason");
  });

  it("renders nothing when the harness reports no fill and no usage", async () => {
    await mount(undefined);
    expect(host.querySelector('[data-testid="usage-context-bubble"]')).toBeNull();
    expect(host.querySelector(".ct-usage-bubble")).toBeNull();
    expect(host.textContent).toBe("");
  });
});
