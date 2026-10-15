import { describe, expect, it, vi } from "vitest";
import { buildTeamActions, requestTeamMember, teamCallerClaim } from "./teamActions";

describe("team caller claims", () => {
  it("reads current nonces for attributed actions and closes with human authority", async () => {
    const nonces: Record<string, string | undefined> = { first: "nonce-first", second: "nonce-second" };
    const inventory = { removed: [], kept: [], orphan_branches: ["lane/retained"] };
    const invoke = vi.fn(async (_command: string, _args: Record<string, unknown>) => ({ inventory }));
    const actions = buildTeamActions({ invoke, callerNonce: id => nonces[id] });
    await actions.onRenameTeam("team", "Name", "second");
    expect(invoke).toHaveBeenLastCalledWith("mux_rename_team", { teamId: "team", name: "Name", callerTabId: "second", sessionBindingNonce: "nonce-second" });
    nonces.second = "nonce-rebound";
    await actions.onAddTeamMember("team", "worker", "provider", "second", "model");
    expect(invoke).toHaveBeenLastCalledWith("mux_add_team_member", { teamId: "team", role: "worker", provider: "provider", model: "model", callerTabId: "second", sessionBindingNonce: "nonce-rebound" });
    expect(await actions.onCloseTeam("team")).toBe(inventory);
    expect(invoke).toHaveBeenLastCalledWith("mux_close_team", { teamId: "team", forceBusy: true });
    await actions.onCloseTeam("team");
    expect(invoke.mock.calls[invoke.mock.calls.length - 1]).toEqual(["mux_close_team", { teamId: "team", forceBusy: true }]);
  });

  it("refuses every attributed action when its own nonce is missing", async () => {
    const invoke = vi.fn();
    const actions = buildTeamActions({ invoke, callerNonce: id => id === "other" ? "foreign-nonce" : undefined });
    await expect(actions.onRenameTeam("team", "Name", "missing")).rejects.toThrow("current session binding nonce");
    await expect(actions.onAddTeamMember("team", "planner", "provider", "missing")).rejects.toThrow("current session binding nonce");
    expect(invoke).not.toHaveBeenCalled();
    expect(() => teamCallerClaim(null, "orphan")).toThrow();
  });

  it("preserves headerless owner recovery and opaque member briefs", async () => {
    const invoke = vi.fn(async () => undefined);
    const brief = 'opaque\n`$()`';
    await requestTeamMember(invoke, { teamId: "team", role: "cartographer", providerId: "provider", managerTabId: "manager", sessionBindingNonce: "exact", brief });
    expect(invoke).toHaveBeenLastCalledWith("mux_add_team_member", { teamId: "team", role: "cartographer", provider: "provider", brief, callerTabId: "manager", sessionBindingNonce: "exact" });
    await requestTeamMember(invoke, { teamId: "team", role: "orchestrator", providerId: "provider", managerTabId: null });
    expect(invoke).toHaveBeenLastCalledWith("mux_add_team_member", { teamId: "team", role: "orchestrator", provider: "provider" });
  });

  it("surfaces daemon refusals and missing close inventory", async () => {
    const refusal = new Error("Forbidden (403)");
    const actions = buildTeamActions({ invoke: async () => { throw refusal; }, callerNonce: () => "nonce" });
    await expect(actions.onRenameTeam("team", "Name", "caller")).rejects.toBe(refusal);
    await expect(actions.onAddTeamMember("team", "worker", "provider", "caller")).rejects.toBe(refusal);
    const invalid = buildTeamActions({ invoke: async () => undefined, callerNonce: () => "nonce" });
    await expect(invalid.onCloseTeam("team")).rejects.toThrow("lane inventory");
  });
});
