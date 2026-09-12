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
});
