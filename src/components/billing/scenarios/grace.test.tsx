// Scenario: a paid subscription whose payment failed but is still inside its grace period.
// Access stays unlimited and the plan line carries the past_due status so the user can act on it.
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../lib/webApi", async (actual) => ({ ...await actual<object>(), localFetch: vi.fn() }));

import { closeScenario, openScenario, PLAN_PAYLOADS } from "./scenario";

afterEach(closeScenario);

describe("paid subscription in grace", () => {
  it("stays unlimited and shows its status", async () => {
    const scenario = await openScenario(PLAN_PAYLOADS.grace, { workspaces: 2, teams: 6, tabs: 35 });
    expect(scenario.tray).toBe("Pro · past_due");
    await scenario.createWorkspace("/new/third");
    await scenario.createTab();
    await scenario.createPair();
    expect(scenario.modalLine()).toBeNull();
    expect(scenario.daemon.counts()).toEqual({ workspaces: 3, teams: 7, "agent.tabs": 38 });
  });
});
