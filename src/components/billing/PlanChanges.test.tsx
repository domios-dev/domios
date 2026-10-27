import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { PlanCards } from "./PlanCards";
import { PlanChangeDialog } from "./PlanChangeDialog";
import { parseDesktopCatalog } from "./desktopBilling";
import { desktopCatalogFixture } from "./desktopBilling.testdata";
import { AccountLimits } from "./AccountLimits";
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

it("starts the eligible trial and changes a paid tier through separate handlers", async () => {
  const catalog = parseDesktopCatalog(desktopCatalogFixture);
  const choose = vi.fn();
  await render(<PlanCards catalog={catalog} interval="month" onChoose={choose} />);
  await act(async () => button("Start 7-day Pro trial").click());
  expect(choose).toHaveBeenCalledExactlyOnceWith("pro_monthly");
  const change = vi.fn();
  await act(async () => mounted!.root.render(<PlanCards catalog={catalog} interval="month" currentPlan="max" subscribedPlan="max" onManage={vi.fn()} onChangePlan={change} />));
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

it("displays all five signed maxima for a future tier without catalog inference", async () => {
  const model = accountPlanModel({ signed_tier: "future-tier", entitlements: { plan: "pro", status: "active", current_period_end: 0, cancel_at_period_end: false },
    access: { authority: "billing", full_access: false }, stale: false,
    limits: { enforcing: true, workspaces: 19, teams: 8, "agent.tabs": 64, "mesh.machines": 9, "mem.items": 4321 } });
  await render(<><span>{model.plan}</span><AccountLimits limits={model.limits} /></>);
  expect(document.body.textContent).toContain("future-tier");
  expect(Array.from(document.querySelectorAll("dd"), node => node.textContent)).toEqual(["19", "8", "64", "9", "4321"]);
});
