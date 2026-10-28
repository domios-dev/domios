// Scenario: a free installation under enforcement. 2 workspaces (the General workspace does not count),
// 6 Teams across all workspaces and 35 tabs at once; the next one opens the plans modal instead.
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../lib/webApi", async (actual) => ({ ...await actual<object>(), localFetch: vi.fn() }));

import { closeScenario, PROMPTS, openScenario, PLAN_PAYLOADS } from "./scenario";

afterEach(closeScenario);

describe("free", () => {
  it("allows a second workspace beside General, then opens the modal", async () => {
    const scenario = await openScenario(PLAN_PAYLOADS.free, { workspaces: 1 });
    expect(scenario.tray?.tier).toBe("free");
    await scenario.createWorkspace("/new/second");
    expect(scenario.modalPrompt()).toBeNull();
    expect(scenario.app.state().workspaces).toHaveLength(3);
    await scenario.createWorkspace("/new/third");
    expect(scenario.modalPrompt()).toEqual(PROMPTS.workspaces);
    expect(scenario.daemon.created("mux_create_workspace")).toBe(1);
  });

  it("opens the modal at 6 Teams and at 35 tabs", async () => {
    const teams = await openScenario(PLAN_PAYLOADS.free, { workspaces: 1, teams: 6, tabs: 2 });
    await teams.createPair();
    expect(teams.modalPrompt()).toEqual(PROMPTS.teams);
    await closeScenario();
    const tabs = await openScenario(PLAN_PAYLOADS.free, { workspaces: 1, tabs: 35 });
    await tabs.createTab();
    expect(tabs.modalPrompt()).toEqual(PROMPTS.tabs);
    expect(tabs.daemon.created("mux_create_tab")).toBe(0);
  });
});
