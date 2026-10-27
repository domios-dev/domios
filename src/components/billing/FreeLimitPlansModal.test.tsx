import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const fetches = vi.hoisted(() => ({ localFetch: vi.fn(), localJson: vi.fn(async () => ({ consent: "off" })) }));
const account = vi.hoisted(() => ({
  signedIn: true,
  plan: "free",
  paid: true,
  login: null as null | ((outcome: "ok" | "fail") => void),
  logins: 0,
}));
vi.mock("../../lib/webApi", async (actual) => ({ ...await actual<object>(), ...fetches }));

vi.mock("./useDesktopBilling", () => ({ useDesktopBilling: () => ({
  loading: false, subscription: { paid_subscription: account.signedIn && account.paid && account.plan !== "free" ? { tier: account.plan } : null },
}) }));

import type { ToastInput } from "../../lib/notices";
import { connectNoticeToasts } from "../../lib/notices";
import { PlanCatalogProvider } from "./planCatalogContext";
import type { PlanCatalog } from "./planCatalog";
import { FreeLimitPlansModal } from "./FreeLimitPlansModal";
import { checkoutTheme } from "./checkoutTheme";
import { AlreadySubscribedError, freeLimitGate, upgradePlan } from "./freeLimitGate";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let disconnectToasts: () => void = () => {};
const toasts: ToastInput[] = [];

const json = (body: unknown) => new Response(JSON.stringify(body), { status: 200 });
const identity = { uid: "user", display_name: "Ada", email: "ada@example.test", provider: "fake" };
const status = () => ({ identity: account.signedIn ? identity : null, pending: false, last_sign_in_failure: null });

async function fakeDaemon(url: string) {
  const path = url.split("?")[0];
  if (path.endsWith("/status")) return json(status());
  if (path.includes("/plan")) {
    const plan = account.signedIn ? account.plan : "free";
    return json({ entitlements: { plan, status: "active", current_period_end: 0, cancel_at_period_end: false },
      access: { authority: plan === "free" ? "free" : "billing", full_access: false }, stale: false,
      limits: { enforcing: true, workspaces: 3, teams: 6, "agent.tabs": 35 } });
  }
  if (path.endsWith("/login")) {
    account.logins++;
    const outcome = await new Promise<"ok" | "fail">((resolve) => { account.login = resolve; });
    if (outcome === "fail") return new Response("opaque", { status: 503 });
    account.signedIn = true;
    return json(status());
  }
  return json({});
}

beforeEach(() => {
  fetches.localJson.mockClear().mockResolvedValue({ consent: "off" });
  Object.assign(account, { signedIn: true, plan: "free", paid: true, login: null, logins: 0 });
  fetches.localFetch.mockImplementation(fakeDaemon);
  toasts.length = 0;
  disconnectToasts = connectNoticeToasts((toast) => { toasts.push(toast); });
});

async function render(upgrade: (planKey: string) => Promise<void>, catalog?: PlanCatalog) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const modal = <FreeLimitPlansModal upgrade={upgrade} />;
  await act(async () => root!.render(catalog ? <PlanCatalogProvider source={{ load: async () => catalog }}>{modal}</PlanCatalogProvider> : modal));
}

const dialog = () => document.querySelector<HTMLElement>("[role=dialog]");
const button = (name: string) => [...document.querySelectorAll("button")].find((candidate) => candidate.textContent === name)!;
const settle = () => act(async () => {});
const click = async (name: string) => {
  await act(async () => { button(name).click(); });
  await settle();
};
const openModal = async (key: Parameters<typeof freeLimitGate.open>[0]) => {
  await act(async () => freeLimitGate.open(key));
  await settle();
};
const proCard = () => dialog()!.querySelector<HTMLElement>("section[aria-label='Pro plan']")!;

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  act(() => freeLimitGate.close());
  freeLimitGate.setLimits(null);
  disconnectToasts();
  fetches.localFetch.mockReset();
  document.body.replaceChildren();
});

describe("plans modal", () => {
  it("stays closed until a limit opens it, then shows that limit with the daemon's maximum", async () => {
    await render(async () => {});
    expect(dialog()).toBeNull();
    freeLimitGate.setLimits({ enforcing: true, max: { workspaces: 2, teams: 6, "agent.tabs": 35 } });
    await act(async () => freeLimitGate.open("workspaces"));
    expect(dialog()).not.toBeNull();
    expect(freeLimitGate.current()).toEqual({ key: "workspaces" });
    expect(freeLimitGate.limits()?.max.workspaces).toBe(3);
  });

  it("records a consented plan view only when the limit modal opens", async () => {
    fetches.localJson.mockResolvedValue({ consent: "on" });
    await render(async () => {});
    expect(fetches.localJson).not.toHaveBeenCalled();
    await openModal("workspaces");
    expect(fetches.localJson.mock.calls).toEqual([
      ["GET", "/api/analytics/status"],
      ["POST", "/api/analytics/event", { event_type: "plan_viewed", event_properties: {} }],
    ]);
  });

  it("Upgrade checks out pro_yearly and closes", async () => {
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await openModal("agent.tabs");
    await click("Upgrade");
    expect(upgrade).toHaveBeenCalledWith("pro_yearly");
    expect(dialog()).toBeNull();
  });

  it("switching to monthly makes Upgrade check out the monthly Pro price", async () => {
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await openModal("agent.tabs");
    await act(async () => dialog()!.querySelector<HTMLElement>("[aria-label='Billed yearly']")!.click());
    await click("Upgrade");
    expect(upgrade).toHaveBeenCalledExactlyOnceWith("pro_monthly");
  });

  it("a Pro subscriber gets Manage subscription instead of any checkout", async () => {
    account.plan = "pro_yearly";
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await openModal("teams");
    expect(button("Choose Sponsor")).toBeUndefined();
    await click("Manage subscription");
    expect(upgrade).not.toHaveBeenCalled();
  });

  it("a checkout refused as already subscribed switches to Manage subscription without an error toast", async () => {
    const upgrade = vi.fn(async () => { throw new AlreadySubscribedError(); });
    await render(upgrade);
    await openModal("teams");
    await click("Choose Sponsor");
    expect(upgrade).toHaveBeenCalledExactlyOnceWith("sponsor_yearly");
    expect(toasts).toHaveLength(0);
    expect(button("Choose Pro")).toBeUndefined();
    expect([...dialog()!.querySelectorAll("button")].filter((candidate) => candidate.textContent === "Manage subscription")).toHaveLength(3);
  });

  it("upgradePlan reports only an exact already_subscribed conflict as such", async () => {
    fetches.localFetch.mockResolvedValueOnce(new Response("already_subscribed", { status: 409 }));
    await expect(upgradePlan("pro_yearly")).rejects.toBeInstanceOf(AlreadySubscribedError);
    fetches.localFetch.mockResolvedValueOnce(new Response("already_subscribed", { status: 503 }));
    await expect(upgradePlan("pro_yearly")).rejects.not.toBeInstanceOf(AlreadySubscribedError);
  });

  it("a chosen card checks out its own key", async () => {
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await openModal("teams");
    await click("Choose Sponsor");
    expect(upgrade).toHaveBeenCalledWith("sponsor_yearly");
  });

  it("a checkout failure stays open with the cards back, and its toast Retry checks out again", async () => {
    const upgrade = vi.fn(async () => { throw new Error("opaque-billing-failure"); });
    await render(upgrade);
    await openModal("teams");
    await click("Upgrade");
    expect(dialog()).not.toBeNull();
    expect(dialog()!.querySelector("[role=alert]")).toBeNull();
    expect(button("Choose Pro").disabled).toBe(false);
    expect(toasts).toHaveLength(1);
    await act(async () => { await toasts[0].local_actions!.find((action) => action.label === "Retry")!.run(); });
    await settle();
    expect(upgrade).toHaveBeenCalledTimes(2);
  });

  it("signed out: Upgrade signs in on the Pro card, then checks out with no second click", async () => {
    account.signedIn = false;
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await openModal("workspaces");
    expect(button("Upgrade").disabled).toBe(false);
    expect(button("Choose Pro").disabled).toBe(false);
    await click("Upgrade");
    expect(account.logins).toBe(1);
    expect(proCard().querySelector("[role=status]")).not.toBeNull();
    expect(button("Upgrade").disabled).toBe(true);
    expect(upgrade).not.toHaveBeenCalled();
    await act(async () => account.login!("ok"));
    await settle();
    expect(upgrade).toHaveBeenCalledExactlyOnceWith("pro_yearly");
    expect(dialog()).toBeNull();
  });

  it("signed out into an account that already has Pro: confirmation, no checkout, Done closes", async () => {
    account.signedIn = false;
    account.plan = "pro";
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await openModal("workspaces");
    await click("Upgrade");
    await act(async () => account.login!("ok"));
    await settle();
    expect(upgrade).not.toHaveBeenCalled();
    expect(dialog()?.querySelector("section[aria-label$=' plan']")).toBeNull();
    expect(document.activeElement).toBe(button("Done"));
    await click("Done");
    expect(dialog()).toBeNull();
  });

  it("signing in: Cancel returns the cards to idle; Not now cancels and closes", async () => {
    account.signedIn = false;
    const upgrade = vi.fn(async () => {});
    await render(upgrade);
    await openModal("workspaces");
    await click("Choose Pro");
    await click("Cancel");
    expect(proCard().querySelector("[role=status]")).toBeNull();
    expect(button("Upgrade").disabled).toBe(false);
    await click("Upgrade");
    await click("Not now");
    expect(dialog()).toBeNull();
    await act(async () => account.login!("ok"));
    await settle();
    expect(upgrade).not.toHaveBeenCalled();
  });

  it("a failed sign-in is a toast with Retry and the modal stays open", async () => {
    account.signedIn = false;
    await render(vi.fn(async () => {}));
    await openModal("workspaces");
    await click("Upgrade");
    await act(async () => account.login!("fail"));
    await settle();
    expect(dialog()).not.toBeNull();
    expect(button("Upgrade").disabled).toBe(false);
    expect(toasts.map((toast) => toast.local_actions?.some((action) => action.label === "Retry"))).toEqual([true]);
  });

  it("Not now and Escape close it and return focus to where the user was", async () => {
    const origin = document.createElement("button");
    document.body.append(origin);
    origin.focus();
    await render(async () => {});
    await act(async () => freeLimitGate.open("workspaces"));
    expect(document.activeElement).not.toBe(origin);
    await click("Not now");
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(origin);
    await act(async () => freeLimitGate.open("workspaces"));
    await act(async () => { dialog()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true })); });
    expect(dialog()).toBeNull();
  });

  it("upgradePlan posts the chosen key with the app's resolved theme and surfaces a refusal", async () => {
    document.documentElement.dataset.ctTheme = "light";
    fetches.localFetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await upgradePlan();
    const [path, init] = fetches.localFetch.mock.calls[0];
    expect(path).toBe("/api/account/upgrade");
    expect(JSON.parse(init.body)).toEqual({ plan: "pro_yearly", theme: "light" });
    document.documentElement.dataset.ctTheme = "dark";
    fetches.localFetch.mockResolvedValueOnce(new Response("refused", { status: 400 }));
    await expect(upgradePlan("sponsor_yearly")).rejects.toThrow("refused");
    expect(JSON.parse(fetches.localFetch.mock.calls[1][1].body)).toEqual({ plan: "sponsor_yearly", theme: "dark" });
  });

  it("checkout theme is light or dark, never system", () => {
    expect(checkoutTheme({ dataset: { ctTheme: "light" } })).toBe("light");
    expect(checkoutTheme({ dataset: { ctTheme: "dark" } })).toBe("dark");
    expect(checkoutTheme({ dataset: {} })).toBeNull();
    expect(checkoutTheme({ dataset: { ctTheme: "system" } })).toBeNull();
  });
});

it("uses the injected catalog checkout key without inferring a tier price", async () => {
  const upgrade = vi.fn(async () => {});
  await render(upgrade, { offers: [{ id: "injected", name: "Injected", tagline: "", features: [], recommended: true,
    prices: { year: { planKey: "opaque-price-key", amount: 123, currency: "USD" } } }] });
  await openModal("workspaces");
  await click("Upgrade");
  expect(upgrade).toHaveBeenCalledExactlyOnceWith("opaque-price-key");
});

it("lets a granted Pro account upgrade instead of offering subscription management", async () => {
  account.plan = "pro"; account.paid = false;
  const upgrade = vi.fn(async () => {});
  await render(upgrade); await openModal("teams");
  expect(button("Manage subscription")).toBeUndefined();
  await click("Choose Pro");
  expect(upgrade).toHaveBeenCalledExactlyOnceWith("pro_yearly");
});
