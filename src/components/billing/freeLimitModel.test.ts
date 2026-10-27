import { describe, expect, it } from "vitest";
import type { MultiplexerState, Tab, Team, Workspace } from "../../lib/types";
import {
  FREE_LIMIT_REACHED,
  freeLimitBlocked,
  isFreeLimitRefusal,
  limitCounts,
  limitHeadline,
  lockedView,
  meshMachinesDetail,
  meshMachinesRefusal,
  parseFreeLimits,
  reachedLimit,
  refusedLimit,
  reusesWorkspace,
  type FreeLimits,
} from "./freeLimitModel";

const enforcingFree: FreeLimits = { enforcing: true, max: { workspaces: 2, teams: 6, "agent.tabs": 35 } };
const refusal = (key: string) => `${FREE_LIMIT_REACHED}: opaque-refusal-body (limit: ${key})`;

describe("free-limit model", () => {
  it("parses the plan payload limits and refuses malformed ones", () => {
    expect(parseFreeLimits({ enforcing: true, workspaces: 2, teams: 6, "agent.tabs": 35 })).toEqual(enforcingFree);
    expect(parseFreeLimits({ enforcing: true, workspaces: null, teams: null, "agent.tabs": null })?.max)
      .toEqual({ workspaces: null, teams: null, "agent.tabs": null });
    expect(parseFreeLimits(undefined)).toBeNull();
    expect(parseFreeLimits({ workspaces: 2 })).toBeNull();
    expect(parseFreeLimits({ enforcing: true, workspaces: -1 })).toBeNull();
  });

  it("counts local items only and exempts the protected General workspace", () => {
    const state = {
      workspaces: [
        { id: "general", protected: true },
        { id: "a" },
        { id: "b" },
      ] as Workspace[],
      teams: [{ id: "t1" }, { id: "peer", remote_peer_name: "laptop" }] as Team[],
      tabs: {
        one: { id: "one" },
        two: { id: "two" },
        mirrored: { id: "mirrored", remote_peer_id: "peer" },
      } as unknown as Record<string, Tab>,
    } satisfies Pick<MultiplexerState, "workspaces" | "tabs" | "teams">;
    expect(limitCounts(state)).toEqual({ workspaces: 2, teams: 1, "agent.tabs": 2 });
  });

  it("reports a full limit only while the daemon enforces", () => {
    const counts = { workspaces: 2, teams: 0, "agent.tabs": 0 };
    expect(reachedLimit(enforcingFree, counts, ["workspaces"])).toBe("workspaces");
    expect(reachedLimit({ ...enforcingFree, enforcing: false }, counts, ["workspaces"])).toBeNull();
    expect(reachedLimit(null, counts, ["workspaces"])).toBeNull();
    expect(reachedLimit(enforcingFree, { ...counts, workspaces: 1 }, ["workspaces"])).toBeNull();
    expect(reachedLimit({ enforcing: true, max: { workspaces: null, teams: null, "agent.tabs": null } }, counts, ["workspaces"])).toBeNull();
    expect(reachedLimit(enforcingFree, { workspaces: 0, teams: 6, "agent.tabs": 35 }, ["teams", "agent.tabs"])).toBe("teams");
  });

  it("reads the refused limit from the daemon text, falling back to the trigger's key", () => {
    expect(refusedLimit(refusal("teams"), "agent.tabs")).toBe("teams");
    expect(refusedLimit(new Error(`Failed to spawn: ${refusal("agent.tabs")}`), "workspaces")).toBe("agent.tabs");
    expect(refusedLimit(`${FREE_LIMIT_REACHED}: opaque-refusal-body`, "workspaces")).toBe("workspaces");
    expect(refusedLimit("workspace name must not be empty", "workspaces")).toBeNull();
    expect(refusedLimit(undefined, "workspaces")).toBeNull();
    expect(isFreeLimitRefusal(freeLimitBlocked("teams"))).toBe(true);
    expect(refusedLimit(freeLimitBlocked("teams"), "workspaces")).toBe("teams");
  });

  it("never reads the account Mesh limit from daemon text", () => {
    expect(refusedLimit(refusal("mesh.machines"), "workspaces")).toBeNull();
    expect(isFreeLimitRefusal(freeLimitBlocked("mesh.machines"))).toBe(false);
  });

  it("reads the Mesh machine limit only from the server's structured refusal and count", () => {
    expect(meshMachinesRefusal({ reason: "mesh.machines", limit: 2, active_count: 2 })).toEqual({ limit: 2, activeCount: 2 });
    expect(meshMachinesRefusal({ reason: "mesh.machines", limit: 5, active_count: 7 })).toEqual({ limit: 5, activeCount: 7 });
    expect(meshMachinesRefusal({ reason: "mesh.machines", limit: 2, active_count: 1 })).toBeNull();
    expect(meshMachinesRefusal({ reason: "mesh.machines", limit: 2 })).toBeNull();
    expect(meshMachinesRefusal({ reason: "mesh.machines", limit: 0, active_count: 0 })).toBeNull();
    expect(meshMachinesRefusal({ reason: "mesh.machines", limit: "2", active_count: 2 })).toBeNull();
    expect(meshMachinesRefusal({ reason: "mesh-allowance", limit: 2, active_count: 2 })).toBeNull();
    expect(meshMachinesRefusal("mesh.machines")).toBeNull();
    expect(meshMachinesRefusal([{ reason: "mesh.machines", limit: 2, active_count: 2 }])).toBeNull();
    expect(meshMachinesRefusal(null)).toBeNull();
  });

  it("explains which actions free a Mesh slot", () => {
    const detail = meshMachinesDetail({ limit: 2, activeCount: 2 });
    expect(detail).toContain("2 active machines");
    expect(detail).toMatch(/Revoking a machine from your account frees its slot; signing out or unpairing locally does not\./);
  });

  it("never blocks a folder the daemon would reuse", () => {
    const workspaces = [{ id: "a", cwd: "C:\\Repo\\App\\" }] as Workspace[];
    expect(reusesWorkspace(workspaces, "c:/repo/app")).toBe(true);
    expect(reusesWorkspace(workspaces, "c:/repo/other")).toBe(false);
    expect(reusesWorkspace(workspaces, undefined)).toBe(false);
  });

  it("names each limit with its maximum", () => {
    expect(limitHeadline("workspaces")).toBe("Your plan includes 2 workspaces.");
    expect(limitHeadline("teams")).toBe("Your plan includes 6 Teams across all workspaces.");
    expect(limitHeadline("agent.tabs")).toBe("Your plan allows 35 tabs running at once.");
    expect(limitHeadline("workspaces", 3)).toBe("Your plan includes 3 workspaces.");
    expect(limitHeadline("mesh.machines", 2)).toBe("Your plan includes 2 machines in your mesh.");
  });
});

describe("lockedView", () => {
  it("is locked only when the daemon marked the item, and names the limit its click opens", () => {
    expect(lockedView("workspaces", { locked: true })).toMatchObject({ locked: true, limit: "workspaces" });
    expect(lockedView("teams", { locked: true })).toMatchObject({ locked: true, limit: "teams" });
    expect(lockedView("agent.tabs", { locked: true })).toMatchObject({ locked: true, limit: "agent.tabs" });
  });

  it.each([[{ locked: false }], [{}], [null], [undefined]])("is open for %j", (item) => {
    expect(lockedView("workspaces", item)).toEqual({ locked: false });
  });

  it("gives each kind its own accessible label", () => {
    const labels = (["workspaces", "teams", "agent.tabs"] as const).map((key) => {
      const view = lockedView(key, { locked: true });
      return view.locked ? view.label : "";
    });
    expect(new Set(labels).size).toBe(3);
    expect(labels.every(Boolean)).toBe(true);
  });
});
