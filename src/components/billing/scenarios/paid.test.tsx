// Scenario: an active paid (Pro) subscription. No free limits apply: creation goes past 2 workspaces,
// 6 Teams and 35 tabs without the plans modal, and the tray shows the paid plan.
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../lib/webApi", async (actual) => ({ ...await actual<object>(), localFetch: vi.fn() }));

import { closeScenario, openScenario, PLAN_PAYLOADS } from "./scenario";

afterEach(closeScenario);

describe("active paid subscription", () => {
  it("shows the paid plan and creates past every free limit", async () => {
    const scenario = await openScenario(PLAN_PAYLOADS.paid, { workspaces: 2, teams: 6, tabs: 35 });
    expect(scenario.tray?.tier).toBe("pro");
    expect(scenario.plan.limits).toEqual({ enforcing: true, max: { workspaces: null, teams: null, "agent.tabs": null } });
    await scenario.createWorkspace("/new/third");
    await scenario.createTab();
    await scenario.createPair();
    expect(scenario.modalPrompt()).toBeNull();
    expect(scenario.daemon.counts()).toEqual({ workspaces: 3, teams: 7, "agent.tabs": 38 });
  });
});
