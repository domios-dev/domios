// Scenario: a subscription past its period end and grace, or cancelled and ended. The free limits return:
// at or over a limit creation is refused with the plans modal, and nothing that already exists is removed.
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../lib/webApi", async (actual) => ({ ...await actual<object>(), localFetch: vi.fn() }));

import { closeScenario, LINES, openScenario, PLAN_PAYLOADS } from "./scenario";

afterEach(closeScenario);

describe("expired subscription", () => {
  it("refuses new items over the free limits and keeps every existing one", async () => {
    const scenario = await openScenario(PLAN_PAYLOADS.expired, { workspaces: 4, teams: 9, tabs: 50 });
    expect(scenario.tray).toBe("Pro · canceled");
    const before = scenario.app.state();
    const existing = { workspaces: before.workspaces.length, teams: before.teams?.length, tabs: Object.keys(before.tabs).length };

    await scenario.createWorkspace("/new/fifth");
    expect(scenario.modalLine()).toBe(LINES.workspaces);
    await scenario.createTab();
    expect(scenario.modalLine()).toBe(LINES.tabs);
    await scenario.createPair();
    expect(scenario.modalLine()).toBe(LINES.teams);

    expect(scenario.daemon.created("mux_create_workspace") + scenario.daemon.created("mux_create_tab")
      + scenario.daemon.created("mux_create_orchestration_pair")).toBe(0);
    const after = scenario.app.state();
    expect({ workspaces: after.workspaces.length, teams: after.teams?.length, tabs: Object.keys(after.tabs).length }).toEqual(existing);
    expect(existing).toEqual({ workspaces: 5, teams: 9, tabs: 50 });
  });

  it("admits up to the limit and refuses exactly at it", async () => {
    const scenario = await openScenario(PLAN_PAYLOADS.expired, { workspaces: 1, tabs: 34 });
    await scenario.createTab();
    expect(scenario.modalLine()).toBeNull();
    expect(scenario.daemon.counts()["agent.tabs"]).toBe(35);
    await scenario.createTab();
    expect(scenario.modalLine()).toBe(LINES.tabs);
    expect(scenario.daemon.counts()["agent.tabs"]).toBe(35);
  });
});
