import { describe, expect, it } from "vitest";
import { createTabsTransport } from "./transport/tabs";
import { createOrchestrationTransport, type OrchestrationClient } from "./transport/orchestration";
import { teamCloseOutcome, startBackgroundClose } from "../components/Sidebar/teamCloseOutcome";

describe("confirmed close UI contracts", () => {
  it("posts force and the exact target confirmation for human workspace and Team closes", async () => {
    const calls: unknown[] = [];
    const client: OrchestrationClient = { json: async <T,>(_method: string, _path: string, body?: unknown) => {
      calls.push(body);
      return {} as T;
    } };
    await createTabsTransport(client).mux_close_workspace({ workspaceId: "workspace", force: true });
    await createOrchestrationTransport(client).mux_close_team({ teamId: "team", forceBusy: true });
    expect(calls).toEqual([
      { force: true, confirm: "workspace" },
      { force: true, confirm: "team", force_busy: true },
    ]);
  });

  it("dismisses before close is invoked and treats retained lanes as success", async () => {
    const events: string[] = [];
    const result = await startBackgroundClose(() => { events.push("dismiss"); }, async () => {
      events.push("close");
      return { removed: [], kept: [{ path: "/lane/work", branch: "lane/work", reason: "dirty" }], orphan_branches: [] };
    });
    expect(events).toEqual(["dismiss", "close"]);
    expect(teamCloseOutcome(result)).toEqual({ kind: "closed_with_preserved_lanes", survivors: [], preserved: ["/lane/work"] });
  });
});
