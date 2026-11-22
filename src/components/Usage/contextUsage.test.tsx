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
  it("shows the reported context fill and cost with the unified bar pill, and its popover holds the gauge, breakdown, and cost", async () => {
    await mount({ ...fill, cost_usd: 0.42 });
    const bubble = host.querySelector<HTMLButtonElement>('[data-testid="usage-context-bubble"] button')!;
    expect(bubble.textContent).toBe("42%");
    expect(bubble.querySelector(".ct-bar-meter-track")).not.toBeNull();
    expect(bubble.querySelector<HTMLElement>(".ct-bar-meter-fill")?.style.width).toBe("42%");
    expect(bubble.querySelector("svg")).toBeNull();
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

  it("renders structured token breakdown and muted reason note inside the context popover", async () => {
    await mount(fill, {
      provider: "acp",
      captured_at: Date.now(),
      limits_reported: false,
      runtime_usage: [{
        tab_id: "acp",
        thread_id: "t",
        billing_mode: "unknown",
        rate_limits: null,
        token_usage: {
          captured_at: Date.now(),
          value: { input_tokens: 17_900, output_tokens: 500, cache_read_tokens: 12_000, cost_micros: 40_000 },
        },
      }],
    } as UsageSnapshot);
    const bubble = host.querySelector<HTMLButtonElement>('[data-testid="usage-context-bubble"] button')!;
    expect(bubble.textContent).toBe("42%");
    await act(async () => bubble.click());
    const breakdown = document.body.querySelector('[aria-label="Token breakdown"]')!;
    expect(breakdown).not.toBeNull();
    expect(breakdown.textContent).toContain("Input17,900");
    expect(breakdown.textContent).toContain("Output500");
    expect(breakdown.textContent).toContain("Cached12,000");
    expect(breakdown.textContent).not.toContain("Cache write");
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

  it("shows ACP session tokens and breakdown even without context fill or API billing mode", async () => {
    await mount(undefined, {
      provider: "acp",
      captured_at: Date.now(),
      limits_reported: false,
      runtime_usage: [{
        tab_id: "acp",
        thread_id: "t",
        billing_mode: "unknown",
        rate_limits: null,
        token_usage: {
          captured_at: Date.now(),
          value: { input_tokens: 25_368, output_tokens: 33, cache_read_tokens: 25_088, cost_micros: 4_523 },
        },
      }],
    } as UsageSnapshot);
    const bubble = host.querySelector<HTMLButtonElement>(".ct-usage-bubble")!;
    expect(bubble.textContent).toBe("25,401 tokens · $0.00");
    expect(bubble.getAttribute("aria-label")).toBe("usage 25,401 tokens · $0.00");
    await act(async () => bubble.click());
    const status = document.body.querySelector('[role="status"]');
    expect(status?.textContent).toContain("Session input: 25368; cached input: 25088; cache write: 0; output: 33; cost: $0.004523");
  });

  it("shows not reported instead of zero when an ACP runtime snapshot has no token usage", async () => {
    await mount(undefined, {
      provider: "acp",
      captured_at: Date.now(),
      limits_reported: false,
      runtime_usage: [{
        tab_id: "acp",
        thread_id: "t",
        billing_mode: "unknown",
        rate_limits: null,
        token_usage: null,
      }],
    } as UsageSnapshot);
    const bubble = host.querySelector<HTMLButtonElement>(".ct-usage-bubble")!;
    expect(bubble.textContent).toBe("—");
    expect(bubble.getAttribute("aria-label")).toBe("usage not reported");
    await act(async () => bubble.click());
    expect(document.body.querySelector('[role="status"]')?.textContent).toBe("Usage not reported");
  });

  it("shows session tokens and keeps the unavailable reason when a profile reports tokens without context fill", async () => {
    bridge.listen.mockResolvedValue(() => undefined);
    bridge.invoke.mockImplementation(async (command: string) => command === "usage_latest" ? ({
      provider: "acp",
      limits_reported: false,
      runtime_usage: [{
        tab_id: "acp",
        thread_id: "t",
        billing_mode: "unknown",
        rate_limits: null,
        token_usage: { captured_at: Date.now(), value: { input_tokens: 1_000, output_tokens: 500, cache_read_tokens: 400 } },
      }],
    }) : null);
    const store = createMuxStateStore(() => ({ tabs: { acp: { id: "acp", context: undefined } } } as never));
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root!.render(<MuxStateStoreContext.Provider value={store}>
      <UnavailableLimitsBubble reason="Opaque reason" session={{ provider: "acp", tabId: "acp", threadId: "t", context: undefined, label: "Grok" }} />
    </MuxStateStoreContext.Provider>));
    await act(async () => { await Promise.resolve(); });
    const bubble = host.querySelector<HTMLButtonElement>(".ct-usage-bubble")!;
    expect(bubble.textContent).toBe("1,500 tokens");
    expect(bubble.getAttribute("aria-label")).toBe("usage 1,500 tokens");
    await act(async () => bubble.click());
    const popover = document.body.querySelector('[role="dialog"][aria-label="usage details"]')!;
    expect(popover.textContent).toContain("Session input: 1000; cached input: 400; cache write: 0; output: 500");
    expect(popover.textContent).toContain("Opaque reason");
  });
});
