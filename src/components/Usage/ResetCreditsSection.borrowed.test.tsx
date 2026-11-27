import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderDescriptor } from "../../lib/types";
import type { RuntimeUsageProjection, UsageSnapshot } from "../../types/usage";
import { UsageMachineContext } from "../../hooks/usageMachine";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import { ResetCreditsSection } from "./ResetCreditsSection";
import {
  ResetCreditsModel,
  clearResetCreditsForTab,
  providerSupportsResetCredits,
  resetCountSummary,
  resolveResetCreditsTarget,
} from "./resetCreditsModel";
import { UsageBubble } from "./UsageBubble";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("../../lib/bridge", async (original) => ({
  ...(await original<typeof import("../../lib/bridge")>()),
  invoke: bridge.invoke,
  listen: bridge.listen,
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CAPABLE_PROVIDER = "opaque-capable-provider";
const READONLY_PROVIDER = "opaque-readonly-provider";

function descriptor(id: string, resetCreditsMinVersion?: string): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: `Display ${id}`,
    transport: "chat_backend",
    system_prompt_delivery: "external",
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
    available_models: [],
    agent_runtime: {
      usage: true,
      ...(resetCreditsMinVersion ? { resetCreditsMinVersion } : {}),
    },
  };
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  host?.remove();
  host = null;
  bridge.invoke.mockReset();
  bridge.listen.mockReset();
  setAgentDescriptors([]);
  clearResetCreditsForTab("owner-tab");
  clearResetCreditsForTab("own-tab");
  clearResetCreditsForTab("borrowing-tab");
});

function mountHost(): Root {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  return root;
}

function findButton(label: string): HTMLButtonElement | undefined {
  return [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => (button.getAttribute("aria-label") ?? button.textContent) === label,
  );
}

describe("resetCreditsModel pure target and summary helpers", () => {
  it("formats count-only summaries honestly for singular and plural counts", () => {
    expect(resetCountSummary(1)).toBe("1 reset available · the provider picks which one is used");
    expect(resetCountSummary(2)).toBe("2 resets available · the provider picks which one is used");
  });

  it("gates reset redemption by provider capability rather than provider id", () => {
    expect(providerSupportsResetCredits(descriptor(CAPABLE_PROVIDER, "1.0.0"))).toBe(true);
    expect(providerSupportsResetCredits(descriptor(READONLY_PROVIDER))).toBe(false);
    expect(providerSupportsResetCredits(undefined)).toBe(true);
  });

  it("routes borrowed tabs through the account owner's live tab and refuses when unavailable", () => {
    const ownerReading: RuntimeUsageProjection = {
      tab_id: "owner-tab",
      thread_id: "owner-thread",
      billing_mode: "subscription",
      token_usage: null,
      rate_limits: { captured_at: 100, value: [{ window: "primary", usedPercent: 10 }] },
    };
    expect(resolveResetCreditsTarget({
      tabId: "borrowing-tab",
      threadId: "borrowing-thread",
      borrowedAccount: true,
      accountReading: ownerReading,
      providerSupportsResets: true,
    })).toEqual({ tabId: "owner-tab", threadId: "owner-thread", readOnlyReason: null });

    expect(resolveResetCreditsTarget({
      tabId: "borrowing-tab",
      threadId: "borrowing-thread",
      borrowedAccount: true,
      accountReading: ownerReading,
      providerSupportsResets: false,
    })).toMatchObject({ tabId: null, readOnlyReason: expect.any(String) });

    expect(resolveResetCreditsTarget({
      tabId: "borrowing-tab",
      threadId: "borrowing-thread",
      peerId: "remote-peer",
      borrowedAccount: true,
      accountReading: ownerReading,
      providerSupportsResets: true,
    })).toMatchObject({ tabId: null, readOnlyReason: expect.any(String) });

    expect(resolveResetCreditsTarget({
      tabId: "borrowing-tab",
      threadId: "borrowing-thread",
      borrowedAccount: true,
      accountReading: null,
      providerSupportsResets: true,
    })).toMatchObject({ tabId: null, readOnlyReason: expect.any(String) });
  });
});

describe("ResetCreditsSection count-only, per-credit, and read-only rendering", () => {
  it("renders count-only resets as one honest summary line without a pseudo-card list", async () => {
    const r = mountHost();
    const model = new ResetCreditsModel(() => "12345678-1234-4123-8123-123456789abc", async () => ({ outcome: "reset" }));
    await act(async () => r.render(
      <ResetCreditsSection resets={{ availableCount: 2, nextExpiry: null, credits: null }} model={model} />,
    ));
    const section = host!.querySelector('section[aria-label="Session resets"]')!;
    expect(section.querySelector("ul")).toBeNull();
    expect(section.querySelector("li")).toBeNull();
    expect(section.textContent).toContain("2 resets available · the provider picks which one is used");
    expect(section.textContent).not.toContain("Chosen by provider");
    expect(findButton("Use Session reset")).toBeDefined();
  });

  it("renders one row per credit when per-credit details are reported", async () => {
    const r = mountHost();
    const model = new ResetCreditsModel(() => "12345678-1234-4123-8123-123456789abc", async () => ({ outcome: "reset" }));
    await act(async () => r.render(
      <ResetCreditsSection
        resets={{
          availableCount: 2,
          nextExpiry: null,
          credits: [
            { id: "c1", title: "Alpha grant", status: "available", expiresAt: null },
            { id: "c2", title: "Beta grant", status: "available", expiresAt: null },
          ],
        }}
        model={model}
      />,
    ));
    const items = host!.querySelectorAll('section[aria-label="Session resets"] li');
    expect(items.length).toBe(2);
    expect(findButton("Use Alpha grant")).toBeDefined();
    expect(findButton("Use Beta grant")).toBeDefined();
  });

  it("renders read-only state with visibly muted marker, badge, no action buttons, and a short reason", async () => {
    const r = mountHost();
    await act(async () => r.render(
      <ResetCreditsSection
        resets={{ availableCount: 2, nextExpiry: null, credits: null }}
        model={null}
        readOnlyReason="Opaque read-only explanation."
      />,
    ));
    const section = host!.querySelector('section[aria-label="Session resets"]')!;
    expect(section.getAttribute("data-readonly")).toBe("true");
    expect(section.classList.contains("is-readonly")).toBe(true);
    expect(section.querySelector("ul")).toBeNull();
    expect(section.querySelectorAll("button").length).toBe(0);
    expect(section.textContent).toContain("Read-only");
    expect(section.textContent).toContain("2 resets available · the provider picks which one is used");
    expect(section.textContent).toContain("Opaque read-only explanation.");
  });
});

describe("UsageBubble session resets across own-tab and borrowed-account states", () => {
  function setupInvoke(snapshot: UsageSnapshot) {
    const resetCalls: Array<{ tabId: unknown; request: Record<string, unknown> }> = [];
    bridge.listen.mockResolvedValue(() => undefined);
    bridge.invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === "agent_runtime_reset_credits") {
        resetCalls.push({ tabId: args?.tabId, request: args?.request as Record<string, unknown> });
        return { outcome: "reset" };
      }
      if (command === "usage_latest") return snapshot;
      if (command === "provider_model_usage" || command === "usage_history") return [];
      if (command === "get_config") return { ui: { usage_intro_hint_opens: 5 } };
      return null;
    });
    return resetCalls;
  }

  it("redeems a count-only reset from a borrowed tab through the account owner's runtime in two clicks and never touches the borrowing tab", async () => {
    setAgentDescriptors([descriptor(CAPABLE_PROVIDER, "1.0.0")]);
    const now = Date.now();
    const ownerReading: RuntimeUsageProjection = {
      tab_id: "owner-tab",
      thread_id: "owner-thread",
      billing_mode: "subscription",
      token_usage: null,
      rate_limits: { captured_at: now, value: [{ window: "primary", usedPercent: 41 }, { window: "secondary", usedPercent: 17 }] },
      resets: { availableCount: 2, nextExpiry: null, credits: null },
    };
    const snapshot = {
      provider: CAPABLE_PROVIDER,
      captured_at: now - 1,
      parsed_ok: true,
      runtime_usage: [ownerReading],
    } as UsageSnapshot;
    const resetCalls = setupInvoke(snapshot);

    const r = mountHost();
    await act(async () => r.render(
      <UsageBubble
        provider={CAPABLE_PROVIDER}
        tabId="borrowing-tab"
        threadId="borrowing-thread"
        borrowedAccount
        trackingEnabled
      />,
    ));

    await act(async () => host!.querySelector<HTMLButtonElement>("button")!.click());
    const dialog = document.querySelector('[role="dialog"][aria-label="usage details"]')!;
    expect(dialog.textContent).toContain("2 resets available · the provider picks which one is used");
    expect(dialog.querySelector('section[aria-label="Session resets"] ul')).toBeNull();
    expect(resetCalls).toEqual([{ tabId: "owner-tab", request: { action: "read" } }]);

    await act(async () => findButton("Use Session reset")!.click());
    await act(async () => findButton("Use reset")!.click());

    expect(resetCalls.some((c) => c.tabId === "borrowing-tab")).toBe(false);
    expect(resetCalls[1]).toMatchObject({
      tabId: "owner-tab",
      request: { action: "redeem", confirmed: true },
    });
    expect(resetCalls[2]).toEqual({ tabId: "owner-tab", request: { action: "read" } });
  });

  it("redeems a selected per-credit reset on an own tab through its own runtime", async () => {
    setAgentDescriptors([descriptor(CAPABLE_PROVIDER, "1.0.0")]);
    const now = Date.now();
    const ownReading: RuntimeUsageProjection = {
      tab_id: "own-tab",
      thread_id: "own-thread",
      billing_mode: "subscription",
      token_usage: null,
      rate_limits: { captured_at: now, value: [{ window: "primary", usedPercent: 50 }] },
      resets: {
        availableCount: 2,
        nextExpiry: null,
        credits: [
          { id: "credit-a", title: "First grant", status: "available", expiresAt: null },
          { id: "credit-b", title: "Second grant", status: "available", expiresAt: null },
        ],
      },
    };
    const snapshot = {
      provider: CAPABLE_PROVIDER,
      captured_at: now - 1,
      parsed_ok: true,
      runtime_usage: [ownReading],
    } as UsageSnapshot;
    const resetCalls = setupInvoke(snapshot);

    const r = mountHost();
    await act(async () => r.render(
      <UsageBubble
        provider={CAPABLE_PROVIDER}
        tabId="own-tab"
        threadId="own-thread"
        trackingEnabled
      />,
    ));

    await act(async () => host!.querySelector<HTMLButtonElement>("button")!.click());
    expect(document.querySelectorAll('section[aria-label="Session resets"] li').length).toBe(2);
    await act(async () => findButton("Use Second grant")!.click());
    await act(async () => findButton("Use reset")!.click());

    expect(resetCalls[0]).toEqual({ tabId: "own-tab", request: { action: "read" } });
    expect(resetCalls[1]).toMatchObject({
      tabId: "own-tab",
      request: { action: "redeem", confirmed: true, creditId: "credit-b" },
    });
  });

  it("stays visibly read-only and never invokes reset redemption when the account provider lacks reset capability or is remote", async () => {
    setAgentDescriptors([
      descriptor(READONLY_PROVIDER),
      descriptor(CAPABLE_PROVIDER, "1.0.0"),
    ]);
    const now = Date.now();
    const reading: RuntimeUsageProjection = {
      tab_id: "owner-tab",
      thread_id: "owner-thread",
      billing_mode: "subscription",
      token_usage: null,
      rate_limits: { captured_at: now, value: [{ window: "primary", usedPercent: 25 }] },
      resets: { availableCount: 2, nextExpiry: null, credits: null },
    };

    const unsupportedSnapshot = {
      provider: READONLY_PROVIDER,
      captured_at: now - 1,
      parsed_ok: true,
      runtime_usage: [reading],
    } as UsageSnapshot;
    const unsupportedCalls = setupInvoke(unsupportedSnapshot);

    const r = mountHost();
    await act(async () => r.render(
      <UsageBubble
        provider={READONLY_PROVIDER}
        tabId="borrowing-tab"
        threadId="borrowing-thread"
        borrowedAccount
        trackingEnabled
      />,
    ));
    await act(async () => host!.querySelector<HTMLButtonElement>("button")!.click());
    const unsupportedSection = document.querySelector('section[aria-label="Session resets"]')!;
    expect(unsupportedSection.getAttribute("data-readonly")).toBe("true");
    expect(unsupportedSection.querySelectorAll("button").length).toBe(0);
    expect(unsupportedSection.textContent).toContain("This provider doesn’t support redeeming resets here.");
    expect(unsupportedCalls).toEqual([]);

    const remoteSnapshot = {
      provider: CAPABLE_PROVIDER,
      captured_at: now - 1,
      parsed_ok: true,
      runtime_usage: [reading],
    } as UsageSnapshot;
    const remoteCalls = setupInvoke(remoteSnapshot);
    await act(async () => r.render(
      <UsageMachineContext.Provider value="opaque-peer">
        <UsageBubble
          key="remote"
          provider={CAPABLE_PROVIDER}
          tabId="mesh:opaque-peer/borrowing-tab"
          threadId="borrowing-thread"
          borrowedAccount
          trackingEnabled
        />
      </UsageMachineContext.Provider>,
    ));
    await act(async () => host!.querySelector<HTMLButtonElement>("button")!.click());
    const remoteSection = document.querySelector('section[aria-label="Session resets"]')!;
    expect(remoteSection.getAttribute("data-readonly")).toBe("true");
    expect(remoteSection.querySelectorAll("button").length).toBe(0);
    expect(remoteSection.textContent).toContain("Use resets on the machine running this account.");
    expect(remoteCalls).toEqual([]);
  });
});
