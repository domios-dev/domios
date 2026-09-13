import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { PlanViewShell } from "./PlanViewShell";
import {
  DEFAULT_PLAN_VIEW_MODE,
  PARKED_PLAN_VIEW_MODES,
  PLAN_VIEW_MODES,
  planViewModeLabel,
  resolvePlanViewMode,
} from "./planViewMode";

const props = {
  mode: "workspace" as const,
  plans: [FIXTURE_PLAN, { ...FIXTURE_PLAN, id: "plan-second", title: "Second plan" }],
  tabs: {},
  workspaceId: "ws-test",
  workspaces: [{ id: "ws-test", name: "Test", tabs: ["orch-1"], active_tab_id: null, cwd: null }],
};

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, unmount: () => act(async () => root.unmount()) };
}

function labels(host: HTMLElement) {
  return Array.from(host.querySelectorAll('[data-testid="plan-view-mode-switch"] [role="tab"]')).map((tab) =>
    tab.textContent?.trim(),
  );
}

async function clickMode(host: HTMLElement, label: string) {
  const tab = Array.from(host.querySelectorAll<HTMLButtonElement>('[data-testid="plan-view-mode-switch"] [role="tab"]')).find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  expect(tab).toBeDefined();
  await act(async () => tab?.click());
}

function isSurfaceHidden(host: HTMLElement, testId: string) {
  return host.querySelector(`[data-testid="${testId}"]`)?.parentElement?.hasAttribute("hidden");
}

describe("team plan view shell", () => {
  it("exposes Board, Spec, Plan, Tasks with Board selected by default", async () => {
    localStorage.removeItem("codeterm.planViewMode");
    const { host, unmount } = await render(<PlanViewShell {...props} />);

    expect(labels(host)).toEqual(["Board", "Spec", "Plan", "Tasks"]);
    expect(host.querySelector('[data-testid="plan-view-shell"]')?.getAttribute("data-plan-view-mode")).toBe("board");
    expect(isSurfaceHidden(host, "mission-control-plan-surface")).toBe(false);
    expect(isSurfaceHidden(host, "plan-board-view")).toBe(true);
    expect(host.querySelector('[data-testid="plan-companion-bubble"]')).toBeNull();
    expect(host.querySelector('[data-testid="tab-session-chat"]')).toBeNull();
    await unmount();
  });

  it("draws Board apart from the plan views inside one tablist with a single thumb", async () => {
    localStorage.removeItem("codeterm.planViewMode");
    const { host, unmount } = await render(<PlanViewShell {...props} />);

    const tablist = host.querySelector('[data-testid="plan-view-mode-switch"]') as HTMLElement;
    expect(tablist.getAttribute("role")).toBe("tablist");
    const groups = Array.from(tablist.querySelectorAll("[data-group]")).map((group) => [
      group.getAttribute("data-group"),
      Array.from(group.querySelectorAll('[role="tab"]')).map((tab) => tab.textContent),
    ]);
    expect(groups).toEqual([
      ["board", ["Board"]],
      ["plan", ["Spec", "Plan", "Tasks"]],
    ]);
    expect(tablist.querySelectorAll('[data-testid="plan-view-mode-thumb"]')).toHaveLength(1);
    expect(tablist.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Board");
    await clickMode(host, "Plan");
    expect(tablist.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Plan");
    await unmount();
  });

  it("disables the plan views and stays on Board while the team has no active plan", async () => {
    localStorage.setItem("codeterm.planViewMode", "spec");
    const archived = { ...FIXTURE_PLAN, id: "plan-old", team_id: "team-1", status: "archived" as const };
    const { host, unmount } = await render(<PlanViewShell {...props} plans={[archived]} teamId="team-1" />);

    expect(host.querySelector('[data-testid="plan-view-shell"]')?.getAttribute("data-plan-view-mode")).toBe("board");
    expect(host.querySelector('[data-group="plan"]')?.getAttribute("data-disabled")).toBe("true");
    const tabs = Array.from(host.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
    expect(tabs.map((tab) => [tab.textContent, tab.disabled])).toEqual([
      ["Board", false],
      ["Spec", true],
      ["Plan", true],
      ["Tasks", true],
    ]);
    expect(localStorage.getItem("codeterm.planViewMode")).toBe("spec");
    await unmount();
    localStorage.removeItem("codeterm.planViewMode");
  });

  it("parks Canvas and List and resolves persisted parked values to Board", () => {
    expect(PARKED_PLAN_VIEW_MODES).toEqual(["list", "canvas"]);
    expect(resolvePlanViewMode("list")).toBe(DEFAULT_PLAN_VIEW_MODE);
    expect(resolvePlanViewMode("canvas")).toBe(DEFAULT_PLAN_VIEW_MODE);
    expect(PLAN_VIEW_MODES.map(planViewModeLabel)).toEqual(["Board", "Spec", "Plan", "Tasks"]);
  });

  it("switches document and kanban surfaces and marks the open plan", async () => {
    localStorage.removeItem("codeterm.planViewMode");
    const { host, unmount } = await render(<PlanViewShell {...props} />);

    await clickMode(host, "Spec");
    expect(host.querySelector('[data-testid="plan-view-shell"]')?.getAttribute("data-plan-view-mode")).toBe("spec");
    expect(isSurfaceHidden(host, "plan-spec-surface")).toBe(false);
    await clickMode(host, "Plan");
    expect(isSurfaceHidden(host, "plan-plan-surface")).toBe(false);
    await clickMode(host, "Tasks");
    expect(isSurfaceHidden(host, "plan-board-view")).toBe(false);
    await clickMode(host, "Board");

    const options = host.querySelectorAll<HTMLButtonElement>(
      '[data-testid="mission-control-plan-surface"] [data-testid="plan-switcher-option"]',
    );
    expect(options).toHaveLength(2);
    expect(options[0]?.getAttribute("data-active")).toBe("true");
    await act(async () => options[1]?.click());
    expect(options[0]?.getAttribute("data-active")).toBe("false");
    expect(options[1]?.getAttribute("data-active")).toBe("true");
    await unmount();
  });

  it("puts a team board's actions in the mode row instead of a band of their own", async () => {
    localStorage.removeItem("codeterm.planViewMode");
    const teamPlan = { ...FIXTURE_PLAN, id: "plan-team", team_id: "team-1" };
    const { host, unmount } = await render(<PlanViewShell {...props} plans={[teamPlan]} teamId="team-1" />);

    const slot = host.querySelector('[data-testid="plan-view-header-actions"]') as HTMLElement;
    expect(slot.closest("header")).not.toBeNull();
    expect(host.querySelector('[data-testid="mission-control-surface-header"]')).toBeNull();
    expect(slot.querySelector('[data-testid="mission-control-board-menu-trigger"]')?.tagName).toBe("BUTTON");

    await clickMode(host, "Spec");
    expect(slot.querySelector('[data-testid="mission-control-board-menu-trigger"]')).toBeNull();
    await unmount();
  });

  it("keeps the plan switcher band, with its actions, when the board chooses among plans", async () => {
    localStorage.removeItem("codeterm.planViewMode");
    const { host, unmount } = await render(<PlanViewShell {...props} />);

    const header = host.querySelector('[data-testid="mission-control-surface-header"]') as HTMLElement;
    expect(header.querySelector('[data-testid="plan-switcher-option"]')).not.toBeNull();
    expect(header.querySelector('[data-testid="mission-control-board-menu-trigger"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="plan-view-header-actions"]')?.children).toHaveLength(0);
    await unmount();
  });
});
