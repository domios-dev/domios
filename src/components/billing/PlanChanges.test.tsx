import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { PlanCards } from "./PlanCards";
import { PlanChangeDialog } from "./PlanChangeDialog";
import { parseDesktopCatalog } from "./desktopBilling";
import { desktopCatalogFixture } from "./desktopBilling.testdata";
import { accountPlanModel } from "../Settings/accountPlanModel";

vi.mock("../../lib/webApi", async actual => ({ ...await actual<object>(), localJson: async () => ({ consent: "off" }) }));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: { openUrl: vi.fn() } }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;
afterEach(async () => { if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; } });
async function render(element: React.ReactNode) {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { host, root };
  await act(async () => root.render(element));
}
const button = (label: string) => Array.from(document.querySelectorAll("button")).find(element => element.textContent?.trim() === label)!;

it("chooses an ordinary subscription and changes a paid tier through separate handlers", async () => {
  const catalog = parseDesktopCatalog({ ...desktopCatalogFixture, catalog: { ...desktopCatalogFixture.catalog, mode: "live" } });
  const choose = vi.fn();
  await render(<PlanCards catalog={catalog} interval="month" onChoose={choose} />);
  expect(button("Start 7-day Pro trial")).toBeUndefined();
  await act(async () => button("Choose Pro").click());
  expect(choose).toHaveBeenCalledExactlyOnceWith("pro_monthly");
  const change = vi.fn();
  await act(async () => mounted!.root.render(<PlanCards catalog={catalog} interval="year" subscriptionInterval="month" currentPlan="max" subscribedPlan="max" onManage={vi.fn()} onChangePlan={change} />));
  await act(async () => button("Downgrade to Pro").click());
  expect(change).toHaveBeenCalledExactlyOnceWith("pri_pro", "pro", "downgrade");
});

it("requires explicit confirmation before a payable change", async () => {
  const submit = vi.fn();
  await render(<PlanChangeDialog selection={{ priceId: "pri_max", tier: "max", direction: "upgrade" }} currentTier="pro" periodEnd={null}
    pending={false} submitted={false} error={null} onSubmit={submit} onClose={vi.fn()} />);
  expect(submit).not.toHaveBeenCalled();
  await act(async () => button("Confirm plan change").click());
  expect(submit).toHaveBeenCalledTimes(1);
});

it("keeps all five signed maxima and the future tier without catalog inference", () => {
  const model = accountPlanModel({ entitlements: { plan: "future-tier", status: "active", current_period_end: 0, cancel_at_period_end: false },
    access: { authority: "billing", full_access: false }, stale: false,
    limits: { enforcing: true, workspaces: 19, teams: 8, "agent.tabs": 64, "mesh.machines": 9, "mem.items": 4321 } });
  expect(model.plan).toBe("future-tier");
  expect(model.limits).toEqual({ enforcing: true, max: { workspaces: 19, teams: 8, "agent.tabs": 64 }, meshMax: 9, memoryMax: 4321 });
});

it("disables a tier change when the subscription interval has no target price", async () => {
  const catalog = parseDesktopCatalog({ ...desktopCatalogFixture, catalog: { ...desktopCatalogFixture.catalog, mode: "live" } });
  delete catalog.offers[1].prices.month;
  const change = vi.fn();
  await render(<PlanCards catalog={catalog} interval="year" subscriptionInterval="month" currentPlan="max" subscribedPlan="max" onManage={vi.fn()} onChangePlan={change} />);
  expect(button("Downgrade to Pro").disabled).toBe(true);
  await act(async () => button("Downgrade to Pro").click());
  expect(change).not.toHaveBeenCalled();
});
