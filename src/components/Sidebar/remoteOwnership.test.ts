import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { managedByLabel, namedManagerRole, remoteIdentity, remoteTeamIdentity } from "./remoteOwnership";

function targetOwned(overrides: Partial<Tab> = {}): Tab {
  return {
    id: "worker-local",
    title: "Worker",
    ownership: {
      controller_title: "Puccini",
      controller_tab_id: "af5919cb",
      peer_id: "peer-123",
      peer_name: "Mac-Air",
      team: { id: "team_7d63c175", title: "Remote Tab Portal" },
    },
    ...overrides,
  } as unknown as Tab;
}

describe("canonical remote ownership presentation", () => {
  it("managed-by names the controller, never the peer", () => {
    expect(managedByLabel(targetOwned())).toEqual("managed by Puccini");
  });

  it("shows the peer alone for workers and a named profile for managing roles", () => {
    const worker = targetOwned({ remote_peer_id: "peer-123", remote_peer_name: "BLACKSTAR", role: "Worker" });
    const planner = { ...worker, role: "Manager" as const, role_profile: "planner" };
    const orchestrator = { ...worker, role: "Manager", role_profile: "orchestrator" as const };
    expect(remoteIdentity(worker)).toEqual({ role: null, peer: "@BLACKSTAR" });
    expect(remoteIdentity(planner)).toEqual({ role: "Planner", peer: "@BLACKSTAR" });
    // Stored authority stays Manager + role_profile; this surface is a display label.
    expect(remoteIdentity(orchestrator)).toEqual({ role: "Orchestrator", peer: "@BLACKSTAR" });
    expect(namedManagerRole({ ...worker, role: "Manager", role_profile: "review_lead" })).toEqual("Review Lead");
    expect(namedManagerRole({ ...worker, role: "Manager", role_profile: null })).toEqual(null);
  });

  it("synthesizes one collision-safe remote Team for a transport-local owned worker", () => {
    const team = remoteTeamIdentity(targetOwned())!;
    expect(team.id).toEqual("mesh:peer-123/team_7d63c175");
    expect(team.title).toEqual("Remote Tab Portal");
    expect(team.peerName).toEqual("Mac-Air");
  });

  it("does not synthesize a remote Team for a transport-remote origin projection", () => {
    const projected = targetOwned({ remote_peer_id: "peer-123", remote_tab_id: "worker" });
    expect(remoteTeamIdentity(projected)).toEqual(null);
    expect(managedByLabel(projected)).toEqual("managed by Puccini");
  });

  it("is null for tabs without ownership", () => {
    expect(managedByLabel({ id: "local", title: "x" } as unknown as Tab)).toEqual(null);
    expect(remoteTeamIdentity({ id: "local", title: "x" } as unknown as Tab)).toEqual(null);
  });
});

finish();
