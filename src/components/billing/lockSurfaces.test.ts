import { describe, expect, it } from "vitest";
import { lockedDimClass } from "./freeLimitModel";
import { lockedSurfaces, meshMachineLockedView } from "./lockSurfaces";
import type { MultiplexerState } from "../../lib/types";

const state = (marks: { workspaces?: string[]; tabs?: string[]; teams?: string[] }): Pick<MultiplexerState, "workspaces" | "tabs" | "teams"> => ({
  workspaces: [
    { id: "w1", name: "One", tabs: ["a", "b"], locked: marks.workspaces?.includes("w1") },
    { id: "w2", name: "Two", tabs: ["c"], locked: marks.workspaces?.includes("w2") },
  ],
  tabs: Object.fromEntries(["a", "b", "c"].map((id) => [id, { id, title: id, locked: marks.tabs?.includes(id) }])),
  teams: [{ id: "t1", locked: marks.teams?.includes("t1") }, { id: "t2", locked: marks.teams?.includes("t2") }],
} as never);

describe("which items each surface marks as locked", () => {
  it("marks nothing when the daemon marks nothing", () => {
    expect(lockedSurfaces(state({}))).toEqual({ workspaces: [], tabs: [], teams: [] });
  });

  it("marks exactly the items the daemon marked", () => {
    expect(lockedSurfaces(state({ workspaces: ["w2"], tabs: ["a"], teams: ["t1"] }))).toEqual({ workspaces: ["w2"], tabs: ["a", "c"], teams: ["t1"] });
  });

  it("a locked workspace locks every tab in it", () => {
    expect(lockedSurfaces(state({ workspaces: ["w1"] })).tabs).toEqual(["a", "b"]);
  });

  it("marks the locked Team's tabs without marking unrelated tabs", () => {
    const snapshot = state({ teams: ["t1"] });
    snapshot.tabs.a.team_id = "t1";
    snapshot.tabs.b.team_id = "t2";
    expect(lockedSurfaces(snapshot)).toEqual({ workspaces: [], tabs: ["a"], teams: ["t1"] });
  });

  it("marks only Mesh machines the account model put over the plan limit", () => {
    expect(meshMachineLockedView({ status: "over_limit" }).locked).toBe(true);
    for (const status of ["linked", "unlinked", "revoked", "expired", "blocked", "legacy"]) {
      expect(meshMachineLockedView({ status }).locked).toBe(false);
    }
  });

  it("dims a locked item and leaves an unlocked one alone", () => {
    expect(lockedDimClass({ locked: true })).not.toBe("");
    expect(lockedDimClass({ locked: false })).toBe("");
  });
});
