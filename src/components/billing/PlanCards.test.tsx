import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { siteLinks } from "../../../shared/siteLinks";
import { PlanCards } from "./PlanCards";
import { FREE_LIMITS } from "./planCatalog";
import vectorPacket from "../../../src-tauri/crates/codeterm-entitlements/vectors/vectors.json";

type SignedVector = { exact_payload?: { utf8: string } };
type SignedLimits = Record<string, { max: number | null }>;
const signedFreeLimits = (vectorPacket as unknown as { vectors: SignedVector[] }).vectors
  .flatMap((vector) => vector.exact_payload ? [JSON.parse(vector.exact_payload.utf8) as { authority: string; limits: SignedLimits }] : [])
  .filter((payload) => payload.authority === "free")
  .flatMap((payload) => Object.entries(payload.limits));

const bridge = vi.hoisted(() => ({ openUrl: vi.fn(async () => undefined) }));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: { openUrl: bridge.openUrl } }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;

afterEach(async () => {
  if (mounted) {
    await act(async () => mounted?.root.unmount());
    mounted.host.remove();
    mounted = null;
  }
  bridge.openUrl.mockClear();
});

async function renderPlanCards() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted = { root, host };
  await act(async () => root.render(<PlanCards />));
  return host;
}

const featuresOf = (host: HTMLElement, plan: string) =>
  Array.from(host.querySelectorAll(`section[aria-label="${plan} plan"] li`), (item) => item.textContent);

it("lists the five Free limits and the unlimited Pro equivalents", async () => {
  const host = await renderPlanCards();

  expect(featuresOf(host, "Free")).toEqual([
    "2 machines in your mesh",
    "2 workspaces",
    "1000 memory items",
    "6 Teams across all workspaces",
    "35 tabs running at once",
  ]);
  expect(featuresOf(host, "Pro")).toEqual([
    "Unlimited machines in your mesh",
    "Unlimited workspaces",
    "Unlimited memory items",
    "Unlimited Teams across all workspaces",
    "Unlimited concurrent tabs",
    "Everything in Free",
  ]);
  expect(featuresOf(host, "Sponsor")).toEqual(["Everything in Pro", "Sponsor role in Discord", "Priority support"]);
});

it("shows every limit of the signed free baseline with its compiled value", () => {
  expect(signedFreeLimits.length).toBeGreaterThan(0);
  for (const [key, limit] of signedFreeLimits) {
    expect({ key, max: FREE_LIMITS[key as keyof typeof FREE_LIMITS] }).toEqual({ key, max: limit.max });
  }
});

it("renders Enterprise separately below the priced plan row with only contact content", async () => {
  const host = await renderPlanCards();
  const planRow = host.querySelector('[role="group"][aria-label="Subscription plans"]');
  const enterprise = host.querySelector('section[aria-label="Enterprise plan"]');

  expect(planRow).not.toBeNull();
  expect(Array.from(planRow!.querySelectorAll("h3"), (heading) => heading.textContent)).toEqual(["Free", "Pro", "Sponsor"]);
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
