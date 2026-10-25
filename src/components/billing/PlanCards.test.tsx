import { act, StrictMode } from "react";
import type React from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { siteLinks } from "../../../shared/siteLinks";
import { PlanCards } from "./PlanCards";
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

const featuresOf = (host: HTMLElement, plan: string) =>
  Array.from(host.querySelectorAll(`section[aria-label="${plan} plan"] li`), (item) => item.textContent);

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

it("lists the current tier allowances", async () => {
  const host = await renderPlanCards();

  expect(featuresOf(host, "Free")).toEqual([
    "2 machines in your mesh",
    "2 workspaces",
    "500 memory items",
    "6 Teams across all workspaces",
    "35 tabs running at once",
  ]);
  expect(featuresOf(host, "Pro")).toEqual([
    "5 machines in your mesh",
    "7 workspaces",
    "1500 memory items",
    "10 Teams across all workspaces",
    "70 tabs running at once",
    "5 Neighbors",
  ]);
  expect(featuresOf(host, "Max")).toContain("20 Neighbors");
  expect(featuresOf(host, "Sponsor")).toContain("Unlimited Neighbors");
});

it("shows the policy two Free baseline", () => {
  expect(FREE_LIMITS).toEqual({ "mesh.machines": 2, workspaces: 2, "mem.items": 500, teams: 6, "agent.tabs": 35 });
});

it("renders Enterprise separately below the priced plan row with only contact content", async () => {
  const host = await renderPlanCards();
  const planRow = host.querySelector('[role="group"][aria-label="Subscription plans"]');
  const enterprise = host.querySelector('section[aria-label="Enterprise plan"]');

  expect(planRow).not.toBeNull();
  expect(Array.from(planRow!.querySelectorAll("h3"), (heading) => heading.textContent)).toEqual(["Free", "Pro", "Max", "Sponsor"]);
  expect(enterprise).not.toBeNull();
  expect(planRow!.compareDocumentPosition(enterprise!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(enterprise!.querySelector("h3")?.textContent).toBe("Enterprise");
  expect(Array.from(enterprise!.querySelectorAll("p"), (line) => line.textContent)).toEqual([
    "Running Domios at scale for a big team? Contact us.",
  ]);
  expect(Array.from(enterprise!.querySelectorAll("button"), (button) => button.textContent?.trim())).toEqual(["Contact us"]);
  expect(enterprise!.querySelector("ul, [aria-label='Billed yearly']")).toBeNull();
});

it("opens the Discord link from siteLinks through the external URL opener", async () => {
  const host = await renderPlanCards();
  const enterprise = host.querySelector('section[aria-label="Enterprise plan"]');
  const contact = Array.from(enterprise!.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Contact us");

  await act(async () => contact?.click());

  expect(bridge.openUrl).toHaveBeenCalledExactlyOnceWith(siteLinks.discord);
});

it("puts the yearly switch and the purchase terms under the plans, yearly on by default", async () => {
  const host = await renderPlanCards();
  const plans = host.querySelector('[aria-label="Subscription plans"]')!;
  const toggle = host.querySelector<HTMLElement>('[aria-label="Billed yearly"]')!;
  const footer = Array.from(host.querySelectorAll("p")).find((line) => line.textContent?.includes("Renews automatically, cancel anytime"))!;
  expect(plans.compareDocumentPosition(toggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(plans.compareDocumentPosition(footer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  expect(Array.from(footer.querySelectorAll("a"), (link) => link.textContent)).toEqual(["14-day money-back guarantee", "Terms", "Privacy"]);
  for (const link of Array.from(footer.querySelectorAll("a"))) await act(async () => link.click());
  expect(bridge.openUrl.mock.calls).toEqual([[`${siteLinks.terms}#refunds`], [siteLinks.terms], [siteLinks.privacy]]);
});

it("a paid subscription never starts a checkout; other paid cards and intervals offer Manage subscription", async () => {
  const onChoose = vi.fn();
  const onManage = vi.fn();
  const host = await renderWith(<PlanCards onChoose={onChoose} onManage={onManage} currentPlan="pro_yearly" subscribedPlan="pro_yearly" />);
  const buttons = () => Array.from(host.querySelectorAll("button"), (button) => button.textContent?.trim());
  expect(buttons().filter((label) => label?.startsWith("Choose"))).toEqual([]);
  const sponsor = host.querySelector('section[aria-label="Sponsor plan"]')!;
  expect(sponsor.textContent).toContain("Manage billing and cancellation in your subscription portal.");
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

it("draws changing Pro greeting and re-entry frames for one second, with a visible reduced-motion field", async () => {
  let now = 100;
  let nextFrame = 0;
  const frames = new Map<number, FrameRequestCallback>();
  vi.spyOn(performance, "now").mockImplementation(() => now);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  const dots: number[][] = [];
  let dot: number[] = [];
  const context = {
    globalAlpha: 1,
    setTransform() {},
    clearRect() { dots.length = 0; },
    beginPath() {},
    arc(x: number, y: number, radius: number) { dot = [x, y, radius]; },
    fill() { dots.push([...dot, Math.round(this.globalAlpha * 255)]); },
  };
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    x: 0, y: 0, left: 0, top: 0, right: 256, bottom: 176, width: 256, height: 176, toJSON() {},
  });
  const preference = vi.fn(() => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
  vi.stubGlobal("matchMedia", preference);
  const sample = (elapsed: number, frameTimestamp = elapsed) => {
    now = elapsed;
    const pending = [...frames.values()];
    frames.clear();
    for (const callback of pending) callback(frameTimestamp);
    return structuredClone(dots);
  };
  const peakAlpha = (frame: number[][]) => Math.max(...frame.map(dot => dot[3]));
  try {
    const pro = card(await renderPlanCards(), "Pro");
    const greeting = [sample(110, 95), sample(350), sample(600), sample(900)];
    expect(new Set(greeting.map(frame => JSON.stringify(frame))).size).toBe(4);
    expect(greeting.every(frame => peakAlpha(frame) > 25)).toBe(true);
    const rest = sample(1100);
    expect(frames.size).toBe(0);
    expect(peakAlpha(rest)).toBeGreaterThanOrEqual(30);
    expect(peakAlpha(rest)).toBeLessThan(50);
    for (const releasedAt of [1200, 2400]) {
      now = releasedAt;
      await act(async () => {
        pro.dispatchEvent(new PointerEvent("pointerout", { bubbles: true }));
        pro.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
      });
      const first = sample(releasedAt + 10, releasedAt - 5);
      const middle = sample(releasedAt + 500);
      const late = sample(releasedAt + 800);
      expect(first).not.toEqual(rest);
      expect(middle).not.toEqual(first);
      expect(late).not.toEqual(middle);
      expect(sample(releasedAt + 1000)).toEqual(rest);
      expect(frames.size).toBe(0);
    }
    await act(async () => mounted?.root.unmount());
    mounted?.host.remove();
    mounted = null;
    preference.mockReturnValue({ matches: true, addEventListener() {}, removeEventListener() {} });
    const reducedPro = card(await renderPlanCards(), "Pro");
    const staticFrame = structuredClone(dots);
    await act(async () => { reducedPro.dispatchEvent(new PointerEvent("pointerover", { bubbles: true })); });
    expect(dots).toEqual(staticFrame);
    expect(peakAlpha(staticFrame)).toBeGreaterThanOrEqual(30);
    expect(frames.size).toBe(0);
  } finally {
    vi.restoreAllMocks();
  }
});
