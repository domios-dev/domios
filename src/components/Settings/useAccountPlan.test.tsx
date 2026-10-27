import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { AccountPlan } from "./accountPlanModel";
import { useAccountPlan } from "./useAccountPlan";

const network = vi.hoisted(() => ({ request: vi.fn() }));
vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  localFetch: (url: string, init?: RequestInit) => network.request(url, init),
}));
const bridge = vi.hoisted(() => ({ handlers: new Map<string, () => void>() }));
vi.mock("../../lib/bridge", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/bridge")>(),
  listen: async (event: string, handler: () => void) => {
    bridge.handlers.set(event, handler);
    return () => bridge.handlers.delete(event);
  },
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function plan(authority: "admin" | "billing" | "free", planName: string, status: string): AccountPlan {
  return {
    entitlements: { plan: planName, status, current_period_end: 0, cancel_at_period_end: false },
    access: { authority, full_access: authority === "admin" },
    stale: false,
  };
}

function response(body: unknown = {}, ok = true) {
  return { ok, json: async () => body, text: async () => ok ? "" : "billing refused" };
}

const outcome: { failure?: unknown } = {};

function Harness() {
  const billing = useAccountPlan("user");
  return <div>
    <output aria-label="Current plan">{billing.plan?.plan ?? "none"}</output>
    <output aria-label="Billing action state">{billing.pending ? "Opening subscription" : "Ready"}</output>
    <button type="button" disabled={billing.pending} onClick={() => void billing.manage().then((failure) => { outcome.failure = failure; })}>
      Manage subscription
    </button>
  </div>;
}

let mounted: { host: HTMLDivElement; root: ReturnType<typeof createRoot> } | null = null;

async function mount(planValue: AccountPlan, action: () => Promise<ReturnType<typeof response>> = async () => response()) {
  network.request.mockImplementation((url: string) => url === "/api/account/plan" ? Promise.resolve(response(planValue)) : action());
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted = { host, root };
  await act(async () => root.render(<Harness />));
  return host;
}

const billingCalls = () => network.request.mock.calls.filter(([url]) => url !== "/api/account/plan").map(([url]) => url);
const state = (host: HTMLElement) => host.querySelector('[aria-label="Billing action state"]')?.textContent;

afterEach(async () => {
  if (mounted) {
    await act(async () => mounted?.root.unmount());
    mounted.host.remove();
    mounted = null;
  }
  network.request.mockReset();
  delete outcome.failure;
});

it.each([
  ["an active paid plan", plan("billing", "pro", "active")],
  ["a past-due paid plan without billing authority", plan("free", "pro", "past_due")],
  ["an admin who also has a paid plan", plan("admin", "sponsor", "active")],
  ["a free entitlement with a paid subscription", plan("free", "free", "active")],
] as const)("Manage subscription opens the billing portal for %s", async (_name, planValue) => {
  const host = await mount(planValue);
  await act(async () => host.querySelector("button")?.click());

  expect(billingCalls()).toEqual(["/api/account/portal"]);
  expect(outcome.failure).toBeNull();
});

it("shows the pending state from the click until the portal answers", async () => {
  let answer!: (value: ReturnType<typeof response>) => void;
  const host = await mount(plan("billing", "pro", "active"), () => new Promise((resolve) => { answer = resolve; }));

  await act(async () => host.querySelector("button")?.click());
  expect(state(host)).toBe("Opening subscription");
  expect(host.querySelector("button")?.disabled).toBe(true);

  await act(async () => answer(response()));
  expect(state(host)).toBe("Ready");
  expect(billingCalls()).toEqual(["/api/account/portal"]);
});

it("hands a portal failure back to the caller and clears the pending state", async () => {
  const host = await mount(plan("billing", "pro", "active"), async () => response({}, false));
  await act(async () => host.querySelector("button")?.click());

  expect(outcome.failure).toBeInstanceOf(Error);
  expect(state(host)).toBe("Ready");
});

it("an entitlement change re-reads the plan without a window focus", async () => {
  const host = await mount(plan("billing", "sponsor", "active"));
  expect(host.querySelector('[aria-label="Current plan"]')?.textContent).toBe("sponsor");

  network.request.mockImplementation(() => Promise.resolve(response(plan("billing", "pro", "active"))));
  await act(async () => bridge.handlers.get("entitlement-changed")?.());

  expect(host.querySelector('[aria-label="Current plan"]')?.textContent).toBe("pro");
  expect(network.request).toHaveBeenLastCalledWith("/api/account/plan", expect.objectContaining({ method: "GET" }));
});
