// Scenario: an admin account. Admin authority is unlimited whatever its billing entitlements say,
// so a free billing plan never brings the limits or the plans modal back.
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../lib/webApi", async (actual) => ({ ...await actual<object>(), localFetch: vi.fn() }));

import { closeScenario, openScenario, PLAN_PAYLOADS } from "./scenario";

afterEach(closeScenario);

describe("admin", () => {
  it("is unlimited regardless of billing", async () => {
    const scenario = await openScenario(PLAN_PAYLOADS.admin, { workspaces: 2, teams: 6, tabs: 35 });
    expect(scenario.tray).toBe("Admin · Free · active");
    await scenario.createWorkspace("/new/third");
    await scenario.createTab();
    await scenario.createPair();
    expect(scenario.modalLine()).toBeNull();
    expect(scenario.daemon.counts()).toEqual({ workspaces: 3, teams: 7, "agent.tabs": 38 });
  });
});
