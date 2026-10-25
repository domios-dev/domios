import { limitHeadline } from "../freeLimitModel";
import { act, createElement, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { vi } from "vitest";
import { localFetch } from "../../../lib/webApi";
import type { AccountPlan, PlanModel } from "../../Settings/accountPlanModel";
import { useAccountPlan } from "../../Settings/useAccountPlan";
import { accountTrayModel } from "../../Titlebar/accountTrayModel";
import { fakeDaemon, modalLine, mountApp, seedState, unmountAll, WORKSPACE } from "../freeLimitHarness";

const PERIOD_END = 2_000_000_000;
const FREE_LIMITS = { enforcing: true, workspaces: 2, teams: 6, "agent.tabs": 35 };
const UNLIMITED = { enforcing: true, workspaces: null, teams: null, "agent.tabs": null };

/** `/api/account/plan` payloads as the daemon sends them for each subscription state. */
export const PLAN_PAYLOADS = {
  paid: { entitlements: { plan: "pro", status: "active", current_period_end: PERIOD_END, cancel_at_period_end: false }, access: { authority: "billing", full_access: false }, stale: false, limits: UNLIMITED },
  grace: { entitlements: { plan: "pro", status: "past_due", current_period_end: PERIOD_END, cancel_at_period_end: false }, access: { authority: "billing", full_access: false }, stale: false, limits: UNLIMITED },
  expired: { entitlements: { plan: "pro", status: "canceled", current_period_end: 1_700_000_000, cancel_at_period_end: true }, access: { authority: "free", full_access: false }, stale: false, limits: FREE_LIMITS },
  admin: { entitlements: { plan: "free", status: "active", current_period_end: 0, cancel_at_period_end: false }, access: { authority: "admin", full_access: true }, stale: false, limits: UNLIMITED },
  free: { entitlements: { plan: "free", status: "active", current_period_end: 0, cancel_at_period_end: false }, access: { authority: "free", full_access: false }, stale: false, limits: FREE_LIMITS },
} satisfies Record<string, AccountPlan>;

export const LINES = {
  workspaces: "Your plan includes 2 workspaces.",
  teams: "Your plan includes 6 Teams across all workspaces.",
  tabs: limitHeadline("agent.tabs"),
};

const identity = { uid: "scenario-user", display_name: "Scenario", email: null, provider: "fake" };
const planRoots: Root[] = [];

/** Loads `payload` through the real plan hook, then mounts the real store against a daemon enforcing the same limits. */
export async function openScenario(payload: AccountPlan, seed: Parameters<typeof seedState>[0]) {
  vi.mocked(localFetch).mockImplementation(async () => new Response(JSON.stringify(payload), { status: 200 }));
  const loaded: { plan: PlanModel | null } = { plan: null };
  function PlanProbe() {
    const { plan } = useAccountPlan(identity.uid);
    useEffect(() => { loaded.plan = plan; }, [plan]);
    return null;
  }
  const host = document.createElement("div");
  const planRoot = createRoot(host);
  planRoots.push(planRoot);
  await act(async () => planRoot.render(createElement(PlanProbe)));
  for (let turn = 0; turn < 100 && !loaded.plan; turn++) await act(async () => { await Promise.resolve(); });
  if (!loaded.plan) throw new Error("plan payload never loaded");
  const daemon = fakeDaemon(seedState(seed), loaded.plan.limits);
  const app = await mountApp(daemon);
  const tray = accountTrayModel({ kind: "signed_in", identity }, loaded.plan).plan;
  const pair = () => app.actions().createOrchestrationPair({ orchestratorAgent: "claude", plannerAgent: "claude", workspaceId: WORKSPACE }).catch(() => null);
  return {
    plan: loaded.plan,
    tray,
    daemon,
    app,
    modalLine,
    createTab: () => app.run(() => app.actions().createTab(WORKSPACE)),
    createWorkspace: (cwd: string) => app.run(() => app.actions().createWorkspace("scenario", cwd)),
    createPair: () => app.run(pair),
  };
}

export async function closeScenario() {
  for (const root of planRoots.splice(0)) await act(async () => root.unmount());
  await unmountAll();
  vi.mocked(localFetch).mockReset();
}
