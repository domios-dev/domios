import { act, StrictMode } from "react";
import type React from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { siteLinks } from "../../../shared/siteLinks";
import { PlanCards } from "./PlanCards";
import { emitDotMatrixWave, sampleDotMatrixWave } from "../brand/dotMatrixModel";
import { FREE_LIMITS } from "./planCatalog";

const bridge = vi.hoisted(() => ({ openUrl: vi.fn(async () => undefined) }));
const analytics = vi.hoisted(() => ({ localJson: vi.fn(async () => ({ consent: "off" })) }));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: { openUrl: bridge.openUrl } }));
vi.mock("../../lib/webApi", async (actual) => ({ ...await actual<object>(), ...analytics }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;

afterEach(async () => {
  if (mounted) {
    await act(async () => mounted?.root.unmount());
    mounted.host.remove();
    mounted = null;
  }
  bridge.openUrl.mockClear();
  analytics.localJson.mockClear();
  analytics.localJson.mockResolvedValue({ consent: "off" });
  vi.unstubAllGlobals();
});

async function renderPlanCards(viewRequested = false) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted = { root, host };
  await act(async () => root.render(<StrictMode><PlanCards viewRequested={viewRequested} /></StrictMode>));
  return host;
}

it("records a plan view once under StrictMode with consent and no account properties", async () => {
  analytics.localJson.mockResolvedValue({ consent: "on" });
  await renderPlanCards(true);
  expect(analytics.localJson.mock.calls).toEqual([
    ["GET", "/api/analytics/status"],
    ["POST", "/api/analytics/event", { event_type: "plan_viewed", event_properties: {} }],
  ]);
});

it("does not send a plan view without consent", async () => {
  await renderPlanCards(true);
  expect(analytics.localJson.mock.calls).toEqual([["GET", "/api/analytics/status"]]);
});

it("shows the policy two Free baseline", () => {
  expect(FREE_LIMITS).toEqual({ "mesh.machines": 2, workspaces: 2, "mem.items": 500, teams: 6, "agent.tabs": 35 });
});

it("opens the Discord link from siteLinks through the external URL opener", async () => {
  const host = await renderPlanCards();
  const enterprise = host.querySelector('section[aria-label="Enterprise plan"]');
  const contact = Array.from(enterprise!.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Contact us");

  await act(async () => contact?.click());

  expect(bridge.openUrl).toHaveBeenCalledExactlyOnceWith(siteLinks.discord);
});

it("opens the purchase documents through the external URL port", async () => {
  const host = await renderPlanCards();
  const links = Array.from(host.querySelectorAll("a"));
  expect(links.map(link => link.href)).toEqual([`${siteLinks.terms}#refunds`, siteLinks.terms, siteLinks.privacy]);
  for (const link of links) await act(async () => link.click());
  expect(bridge.openUrl.mock.calls).toEqual([[`${siteLinks.terms}#refunds`], [siteLinks.terms], [siteLinks.privacy]]);
});

it("the interval toggle selects the checkout key", async () => {
  const choose = vi.fn();
  const host = await renderWith(<PlanCards onChoose={choose} />);
  await act(async () => buttonIn(card(host, "Pro"), "Choose Pro")!.click());
  await act(async () => host.querySelector<HTMLElement>('[aria-label="Billed yearly"]')!.click());
  await act(async () => buttonIn(card(host, "Pro"), "Choose Pro")!.click());
  expect(choose.mock.calls).toEqual([["pro_yearly"], ["pro_monthly"]]);
});

it("a paid subscription never starts a checkout; other paid cards and intervals offer Manage subscription", async () => {
  const onChoose = vi.fn();
  const onManage = vi.fn();
  const host = await renderWith(<PlanCards onChoose={onChoose} onManage={onManage} currentPlan="pro_yearly" subscribedPlan="pro_yearly" />);
  await act(async () => host.querySelector<HTMLElement>('[aria-label="Billed yearly"]')!.click());
  const manage = Array.from(host.querySelectorAll("button")).filter((button) => button.textContent?.trim() === "Manage subscription");
  expect(manage).toHaveLength(3);
  for (const button of manage) await act(async () => button.click());
  expect(onManage).toHaveBeenCalledTimes(3);
  expect(onChoose).not.toHaveBeenCalled();
});

async function renderWith(element: React.ReactElement) {
  const host = mounted?.host ?? document.createElement("div");
  if (!mounted) {
    document.body.append(host);
    mounted = { root: createRoot(host), host };
  }
  await act(async () => mounted!.root.render(element));
  return host;
}

const card = (host: HTMLElement, plan: string) => host.querySelector<HTMLElement>(`section[aria-label="${plan} plan"]`)!;
const buttonIn = (scope: HTMLElement, name: string) =>
  Array.from(scope.querySelectorAll("button")).find((button) => button.textContent?.trim() === name);

it("keeps every Choose button live by default, signed out or not", async () => {
  const host = await renderWith(<PlanCards onChoose={() => {}} />);
  expect(buttonIn(card(host, "Pro"), "Choose Pro")?.disabled).toBe(false);
  expect(buttonIn(card(host, "Sponsor"), "Choose Sponsor")?.disabled).toBe(false);
});

it("shows signing in on the chosen offer in either interval, focuses Cancel and returns focus when it ends", async () => {
  const onCancel = vi.fn();
  const signing = { kind: "signing_in", planKey: "pro_monthly", attached: false } as const;
  const host = await renderWith(<PlanCards onChoose={() => {}} canPurchase={false} intent={signing} onCancel={onCancel} />);
  const pro = card(host, "Pro");
  expect(pro.querySelector('[role="status"]')).not.toBeNull();
  expect(buttonIn(pro, "Choose Pro")).toBeUndefined();
  expect(document.activeElement).toBe(buttonIn(pro, "Cancel"));
  expect(buttonIn(card(host, "Sponsor"), "Choose Sponsor")?.disabled).toBe(true);
  await act(async () => buttonIn(pro, "Cancel")!.click());
  expect(onCancel).toHaveBeenCalledOnce();
  await renderWith(<PlanCards onChoose={() => {}} onCancel={onCancel} />);
  expect(document.activeElement).toBe(buttonIn(card(host, "Pro"), "Choose Pro"));
});

it("opening checkout is the busy button on the chosen card", async () => {
  const host = await renderWith(<PlanCards onChoose={() => {}} canPurchase={false} intent={{ kind: "opening", planKey: "sponsor_yearly" }} />);
  const busy = card(host, "Sponsor").querySelector("button");
  expect(busy?.getAttribute("aria-busy")).toBe("true");
  expect(busy?.disabled).toBe(true);
  expect(card(host, "Pro").querySelector('[aria-busy="true"]')).toBeNull();
});

it("an owned plan shows its status in place of the button", async () => {
  const host = await renderWith(<PlanCards onChoose={() => {}} currentPlan="pro" intent={{ kind: "owned", planKey: "pro_yearly", staff: false }} signedInAs="ada@example.test" />);
  const pro = card(host, "Pro");
  expect(pro.querySelector('[role="status"]')).not.toBeNull();
  expect(pro.querySelector("button")).toBeNull();
  expect(buttonIn(card(host, "Sponsor"), "Choose Sponsor")?.disabled).toBe(false);
});

it("mounting account plans without intent does not record a view", async () => {
  analytics.localJson.mockResolvedValue({ consent: "on" });
  await renderPlanCards();
  expect(analytics.localJson).not.toHaveBeenCalled();
});

it("records a view on the visibility callback, once across repeated callbacks", async () => {
  analytics.localJson.mockResolvedValue({ consent: "on" });
  const callbacks: IntersectionObserverCallback[] = [];
  const disconnect = vi.fn();
  const observe = vi.fn();
  vi.stubGlobal("IntersectionObserver", class {
    constructor(callback: IntersectionObserverCallback) { callbacks.push(callback); }
    observe = observe;
    disconnect = disconnect;
  });
  await renderPlanCards();
  expect(observe).toHaveBeenCalled();
  expect(analytics.localJson).not.toHaveBeenCalled();
  const observer = {} as IntersectionObserver;
  await act(async () => callbacks[callbacks.length - 1]([{ isIntersecting: false } as IntersectionObserverEntry], observer));
  expect(analytics.localJson).not.toHaveBeenCalled();
  await act(async () => {
    callbacks[callbacks.length - 1]([{ isIntersecting: true } as IntersectionObserverEntry], observer);
    callbacks[callbacks.length - 1]([{ isIntersecting: true } as IntersectionObserverEntry], observer);
  });
  expect(analytics.localJson.mock.calls).toEqual([
    ["GET", "/api/analytics/status"],
    ["POST", "/api/analytics/event", { event_type: "plan_viewed", event_properties: {} }],
  ]);
  expect(disconnect).toHaveBeenCalled();
});

it("records an explicit subscription plans request after mounting", async () => {
  analytics.localJson.mockResolvedValue({ consent: "on" });
  await renderWith(<PlanCards />);
  expect(analytics.localJson).not.toHaveBeenCalled();
  await renderWith(<PlanCards viewRequested />);
  await renderWith(<PlanCards viewRequested />);
  expect(analytics.localJson.mock.calls).toEqual([
    ["GET", "/api/analytics/status"],
    ["POST", "/api/analytics/event", { event_type: "plan_viewed", event_properties: {} }],
  ]);
});

it("samples release waves with an injected clock and suppresses reduced motion", () => {
  const wave = { releasedAt: 100, originX: 0, originY: 0, maxRadius: 200 };
  expect(emitDotMatrixWave([], wave, "loading", false)).toEqual([wave]);
  expect(emitDotMatrixWave([], wave, "loading", true)).toEqual([]);
  expect(sampleDotMatrixWave(wave, 99, 1000)).toBeNull();
  expect(sampleDotMatrixWave(wave, 600, 1000)).toEqual({ progress: 0.5, radius: 100, fade: 0.5 });
  expect(sampleDotMatrixWave(wave, 1100, 1000)).toBeNull();
});
