import { act, useMemo, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Notification } from "../../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../../store/multiplexer/context";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import { desktopCatalogFixture } from "../billing/desktopBilling.testdata";

const daemon = vi.hoisted(() => {
  const state = {
    identity: null as null | { uid: string; display_name: string; email: string; provider: string },
    pending: false,
    failure: null as null | { stage: "redemption"; error: "provider" },
    account: { plan: "free", fullAccess: false },
    paidPlan: undefined as string | null | undefined,
    subscriptionUnavailable: false,
    catalogUnavailable: false,
    portals: 0,
    logins: 0,
    login: null as null | ((outcome: "ok" | "fail") => void),
    upgrades: [] as string[],
    upgradeFails: false,
    upgradeWait: null as Promise<Response> | null,
    giftRequests: [] as { key: string; expected_uid: string }[],
    giftResponse: null as unknown,
    giftFailure: null as string | null,
    giftWait: null as Promise<Response> | null,
    giftAbortable: false,
  };
  const ok = (body: unknown) => ({ ok: true, json: async () => body, text: async () => "" });
  const status = () => ({ identity: state.identity, pending: state.pending, last_sign_in_failure: state.failure });
  const plan = () => {
    const signedIn = state.identity !== null;
    const current = signedIn ? state.account : { plan: "free", fullAccess: false };
    return {
      entitlements: { plan: current.plan, status: "active", current_period_end: 0, cancel_at_period_end: false },
      access: { authority: current.fullAccess ? "admin" : current.plan === "free" ? "free" : "billing", full_access: current.fullAccess },
      stale: false,
    };
  };
  async function fetch(url: string, init?: RequestInit) {
    const path = url.split("?")[0];
    if (path.endsWith("/status")) return ok(status());
    if (path.endsWith("/plan") || path.endsWith("/plan/refresh")) return ok(plan());
    if (path.endsWith("/gift/redeem")) {
      state.giftRequests.push(JSON.parse(String(init?.body)));
      if (state.giftAbortable) return new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
      });
      if (state.giftWait) return state.giftWait;
      return state.giftFailure ? { ok: false, text: async () => state.giftFailure! } : ok(state.giftResponse);
    }
    if (path.endsWith("/billing")) {
      const operation = JSON.parse(String(init?.body)).operation;
      if (operation === "catalog") return state.catalogUnavailable ? { ok: false, text: async () => "unavailable" } : ok(desktopCatalogFixture);
      if (state.subscriptionUnavailable) return { ok: false, text: async () => "unavailable" };
      const paid = state.paidPlan === undefined ? state.account.plan === "free" ? null : state.account.plan : state.paidPlan;
      return ok({ current_tier: state.account.plan, period_end: 1700604800, pending_change: null, billing_interval: paid ? "year" : null,
        paid_subscription: paid ? { tier: paid, status: "active", period_end: 1700604800 } : null });
    }
    if (path.endsWith("/portal")) { state.portals++; return ok({}); }
    if (path.endsWith("/login")) {
      state.logins++;
      state.pending = true;
      const outcome = await new Promise<"ok" | "fail">((resolve) => { state.login = resolve; });
      state.pending = false;
      state.login = null;
      if (outcome === "fail") {
        state.failure = { stage: "redemption", error: "provider" };
        return { ok: false, text: async () => "opaque" };
      }
      state.failure = null;
      state.identity = { uid: "user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
      return ok(status());
    }
    if (path.endsWith("/upgrade")) {
      state.upgrades.push(JSON.parse(String(init?.body)).plan);
      if (state.upgradeWait) return state.upgradeWait;
      return state.upgradeFails ? { ok: false, text: async () => "opaque-billing-failure" } : ok({});
    }
    return ok({});
  }
  return { state, fetch };
});

vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  localFetch: (url: string, init?: RequestInit) => daemon.fetch(url, init),
}));

import { AccountTab } from "./AccountTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Harness() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = useMemo(() => ({
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
  }), []);
  return <MultiplexerActionsContext.Provider value={actions as unknown as MultiplexerActionsContextType}>
    <AccountTab />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  Object.assign(daemon.state, {
    identity: null, pending: false, failure: null, account: { plan: "free", fullAccess: false },
    logins: 0, login: null, upgrades: [], upgradeFails: false, upgradeWait: null, paidPlan: undefined, subscriptionUnavailable: false, catalogUnavailable: false, portals: 0,
    giftRequests: [], giftResponse: { schema_version: 1, status: "redeemed", plan: "max", duration: "lifetime", expires_at: null }, giftFailure: null, giftWait: null, giftAbortable: false,
  });
});

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  host.remove();
  vi.useRealTimers();
});

async function mount() {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<Harness />));
}

const settle = () => act(async () => { for (let turn = 0; turn < 10; turn++) await Promise.resolve(); });
const card = (plan: string) => host.querySelector<HTMLElement>(`section[aria-label="${plan} plan"]`)!;
const button = (scope: HTMLElement, name: string) =>
  Array.from(scope.querySelectorAll("button")).find((candidate) => candidate.textContent?.trim() === name);
const click = async (scope: HTMLElement, name: string) => {
  await act(async () => button(scope, name)!.click());
  await settle();
};
const finishLogin = async (outcome: "ok" | "fail") => {
  await act(async () => daemon.state.login!(outcome));
  await settle();
};
const toast = () => host.querySelector('[data-testid="notification-toast"]');

it("signed out: Choose starts sign-in on that card, then opens checkout with no second click", async () => {
  await mount();
  expect(button(card("Pro"), "Choose Pro")?.disabled).toBe(false);
  await click(card("Pro"), "Choose Pro");
  expect(daemon.state.logins).toBe(1);
  expect(card("Pro").querySelector('[role="status"]')).not.toBeNull();
  expect(button(card("Sponsor"), "Choose Sponsor")?.disabled).toBe(true);
  await finishLogin("ok");
  expect(daemon.state.upgrades).toEqual(["pro_yearly"]);
  expect(button(card("Pro"), "Choose Pro")?.disabled).toBe(false);
  expect(toast()).toBeNull();
});

it("an account that already has Pro gets the confirmation and no checkout", async () => {
  daemon.state.account = { plan: "pro", fullAccess: false };
  await mount();
  await click(card("Pro"), "Choose Pro");
  await finishLogin("ok");
  expect(daemon.state.upgrades).toEqual([]);
  expect(card("Pro").querySelector('[role="status"]')).not.toBeNull();
  expect(button(card("Pro"), "Choose Pro")).toBeUndefined();
});

it("staff are never sent to checkout, for any plan", async () => {
  daemon.state.account = { plan: "free", fullAccess: true };
  await mount();
  await click(card("Sponsor"), "Choose Sponsor");
  await finishLogin("ok");
  expect(daemon.state.upgrades).toEqual([]);
  expect(card("Sponsor").querySelector('[role="status"]')).not.toBeNull();
  await click(card("Pro"), "Choose Pro");
  expect(daemon.state.upgrades).toEqual([]);
  expect(card("Pro").querySelector('[role="status"]')).not.toBeNull();
});

it("a Pro subscriber choosing Sponsor still goes to checkout", async () => {
  daemon.state.account = { plan: "pro", fullAccess: false };
  await mount();
  await click(card("Sponsor"), "Choose Sponsor");
  await finishLogin("ok");
  expect(daemon.state.upgrades).toEqual(["sponsor_yearly"]);
});

it("Cancel returns to idle at once, and a late sign-in success opens no checkout", async () => {
  await mount();
  await click(card("Pro"), "Choose Pro");
  await click(card("Pro"), "Cancel");
  expect(card("Pro").querySelector('[role="status"]')).toBeNull();
  await finishLogin("ok");
  expect(daemon.state.upgrades).toEqual([]);
  expect(toast()).toBeNull();
});

it("attaches to a sign-in already running elsewhere instead of starting another", async () => {
  daemon.state.pending = true;
  await mount();
  await click(card("Pro"), "Choose Pro");
  expect(daemon.state.logins).toBe(0);
  expect(card("Pro").querySelector('[role="status"]')).not.toBeNull();
  daemon.state.pending = false;
  daemon.state.identity = { uid: "user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  await act(async () => window.dispatchEvent(new Event("domios:account-changed")));
  await settle();
  expect(daemon.state.upgrades).toEqual(["pro_yearly"]);
});

it("a failed sign-in returns the card to idle and its toast Retry resumes the same plan", async () => {
  await mount();
  await click(card("Sponsor"), "Choose Sponsor");
  await finishLogin("fail");
  expect(button(card("Sponsor"), "Choose Sponsor")?.disabled).toBe(false);
  expect(toast()).not.toBeNull();
  await click(toast() as HTMLElement, "Retry");
  expect(daemon.state.logins).toBe(2);
  await finishLogin("ok");
  expect(daemon.state.upgrades).toEqual(["sponsor_yearly"]);
});

it("a checkout failure is a toast whose Retry checks out again", async () => {
  daemon.state.identity = { uid: "user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  daemon.state.upgradeFails = true;
  await mount();
  await settle();
  await click(card("Pro"), "Choose Pro");
  expect(daemon.state.upgrades).toEqual(["pro_yearly"]);
  expect(host.querySelector('[role="alert"]')).toBeNull();
  daemon.state.upgradeFails = false;
  await click(toast() as HTMLElement, "Retry");
  expect(daemon.state.upgrades).toEqual(["pro_yearly", "pro_yearly"]);
});

it("lets a referral or gift recipient subscribe to Pro without changing their signed-in identity", async () => {
  daemon.state.identity = { uid: "invited-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  daemon.state.account = { plan: "pro", fullAccess: false };
  daemon.state.paidPlan = null;
  await mount();
  await click(card("Pro"), "Choose Pro");
  expect(daemon.state.upgrades).toEqual(["pro_yearly"]);
  expect(daemon.state.identity.uid).toBe("invited-user");
  expect(daemon.state.account.plan).toBe("pro");
  expect(daemon.state.logins).toBe(0);
});

it("preserves an already-paying recipient and directs management to their existing subscription", async () => {
  daemon.state.identity = { uid: "paid-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  daemon.state.account = { plan: "pro", fullAccess: false };
  daemon.state.paidPlan = "pro";
  await mount();
  expect(button(card("Pro"), "Choose Pro")).toBeUndefined();
  await click(host, "Manage subscription");
  expect(daemon.state.portals).toBe(1);
  expect(daemon.state.upgrades).toEqual([]);
  expect(daemon.state.identity.uid).toBe("paid-user");
});

it("does not offer checkout when actual subscription status could not be verified", async () => {
  daemon.state.identity = { uid: "invited-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  daemon.state.account = { plan: "pro", fullAccess: false };
  daemon.state.paidPlan = null;
  daemon.state.subscriptionUnavailable = true;
  await mount();
  const choices = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).filter(element => element.textContent?.startsWith("Choose "));
  expect(choices.length).toBeGreaterThan(0);
  for (const choose of choices) {
    expect(choose.disabled).toBe(true);
  }
  expect(host.querySelector('[role="alert"]')).not.toBeNull();
  expect(daemon.state.upgrades).toEqual([]);
});

it("does not apply a previous account's late checkout refusal to the next account", async () => {
  let finish!: (response: Response) => void;
  daemon.state.upgradeWait = new Promise(resolve => { finish = resolve; });
  daemon.state.identity = { uid: "previous", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  await mount();
  await click(card("Pro"), "Choose Pro");
  daemon.state.identity = { uid: "current", display_name: "Grace", email: "grace@example.test", provider: "fake" };
  await act(async () => window.dispatchEvent(new Event("domios:account-changed")));
  await settle();
  await act(async () => finish(new Response("already_subscribed", { status: 409 })));
  expect(button(card("Pro"), "Choose Pro")?.disabled).toBe(false);
  expect(daemon.state.upgrades).toEqual(["pro_yearly"]);
  expect(daemon.state.identity.uid).toBe("current");
});

it("does not substitute built-in prices when a signed-in account's catalog is unavailable", async () => {
  daemon.state.identity = { uid: "invited-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  daemon.state.catalogUnavailable = true;
  await mount();
  expect(host.querySelector('[role="group"][aria-label="Subscription plans"]')).toBeNull();
  expect(host.querySelector('[role="alert"]')).not.toBeNull();
  expect(daemon.state.upgrades).toEqual([]);
});

const giftForm = () => host.querySelector<HTMLFormElement>('form[aria-label="Redeem a gift key"]');
const giftKey = `g1_${"a".repeat(43)}`;
async function enterGift() {
  const input = giftForm()!.querySelector("input")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, giftKey);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function submitGift() {
  await act(async () => { giftForm()!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })); });
}

it("only signed-in accounts have the accessible gift control", async () => {
  await mount();
  expect(giftForm()).toBeNull();
  daemon.state.identity = { uid: "gift-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  await act(async () => window.dispatchEvent(new Event("domios:account-changed")));
  await settle();
  const input = giftForm()!.querySelector("input")!;
  expect(giftForm()!.querySelector("label")!.htmlFor).toBe(input.id);
  expect(input.type).toBe("password");
  expect(button(giftForm()!, "Redeem gift key")!.disabled).toBe(true);
});

it("displays server lifetime readback without turning a receipt into paid access", async () => {
  daemon.state.identity = { uid: "gift-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  await mount();
  await enterGift();
  await submitGift();
  expect(daemon.state.giftRequests).toEqual([{ key: giftKey, expected_uid: "gift-user" }]);
  expect(giftForm()!.querySelector("input")!.value).toBe("");
  expect(giftForm()!.querySelector('[role="status"]')!.textContent).toContain("Max");
  expect(giftForm()!.querySelector('[role="status"]')!.textContent).toContain("Lifetime");
  expect(giftForm()!.querySelector('[role="status"]')!.textContent).toContain("Free");
  expect(daemon.state.account.plan).toBe("free");
  expect(daemon.state.upgrades).toEqual([]);
  expect(host.textContent).not.toContain(giftKey);
});

it.each(["invalid_key", "already_lifetime", "rate_limited", "unavailable", "opaque-private-material"])("gift refusal %s preserves a paid account and permits an explicit retry", async reason => {
  daemon.state.identity = { uid: "paid-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  daemon.state.account = { plan: "pro", fullAccess: false };
  daemon.state.giftFailure = reason;
  await mount();
  await enterGift();
  await submitGift();
  expect(giftForm()!.querySelector('[role="alert"]')).not.toBeNull();
  expect(host.textContent).not.toContain("opaque-private-material");
  expect(daemon.state.account.plan).toBe("pro");
  expect(daemon.state.giftRequests).toHaveLength(1);
  daemon.state.giftFailure = null;
  await enterGift();
  await submitGift();
  expect(daemon.state.giftRequests).toHaveLength(2);
  expect(giftForm()!.querySelector('[role="status"]')).not.toBeNull();
});

it("a pending gift submission is sent once and a late result cannot cross accounts", async () => {
  let finish!: (response: Response) => void;
  daemon.state.giftWait = new Promise(resolve => { finish = resolve; });
  daemon.state.identity = { uid: "previous", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  await mount();
  await enterGift();
  await submitGift();
  await submitGift();
  expect(daemon.state.giftRequests).toHaveLength(1);
  expect(giftForm()!.querySelector("input")!.disabled).toBe(true);
  daemon.state.identity = { uid: "current", display_name: "Grace", email: "grace@example.test", provider: "fake" };
  await act(async () => window.dispatchEvent(new Event("domios:account-changed")));
  await settle();
  await act(async () => finish(new Response(JSON.stringify(daemon.state.giftResponse), { status: 200 })));
  expect(giftForm()!.querySelector('[role="status"]')).toBeNull();
  expect(giftForm()!.querySelector("input")!.value).toBe("");
  expect(daemon.state.identity.uid).toBe("current");
});

it("an unknown gift response is refused without changing account access", async () => {
  daemon.state.identity = { uid: "gift-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  daemon.state.giftResponse = { schema_version: 1, status: "redeemed", plan: "admin", duration: "lifetime", expires_at: null };
  await mount();
  await enterGift();
  await submitGift();
  expect(giftForm()!.querySelector('[role="alert"]')).not.toBeNull();
  expect(giftForm()!.querySelector('[role="status"]')).toBeNull();
  expect(daemon.state.account).toEqual({ plan: "free", fullAccess: false });
});

it("bounds a stalled redemption without automatically replaying the key", async () => {
  daemon.state.identity = { uid: "gift-user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
  daemon.state.giftAbortable = true;
  await mount();
  vi.useFakeTimers();
  await enterGift();
  await submitGift();
  expect(giftForm()!.querySelector("input")!.disabled).toBe(true);
  await act(async () => vi.advanceTimersByTimeAsync(45_000));
  expect(giftForm()!.querySelector("input")!.disabled).toBe(false);
  expect(giftForm()!.querySelector('[role="alert"]')).not.toBeNull();
  expect(daemon.state.giftRequests).toHaveLength(1);
  expect(daemon.state.account.plan).toBe("free");
});
