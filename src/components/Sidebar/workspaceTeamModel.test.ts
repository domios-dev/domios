import { describe, expect, it } from "vitest";
import { partitionTeamMembers, type TeamTabMember } from "./workspaceTeamModel";
import type { Tab } from "../../lib/types";

function member(id: string, overrides: Partial<Tab> = {}, index = 0): TeamTabMember {
  return { tab: { id, title: id, team_id: "team", parent_tab_id: null, role: null, ...overrides } as Tab, index };
}

const surface = (id: string, owner: string | null): TeamTabMember =>
  member(id, {
    parent_tab_id: owner,
    backing: { kind: "surface", plugin_id: "dash", surface_id: "board", instance_id: id },
  });
const ids = (members: readonly TeamTabMember[]) => members.map(({ tab }) => tab.id);

describe("partitionTeamMembers", () => {
  it("keeps structural roles and owner surfaces together", () => {
    const { managers, terminals, workers, surfacesByOwner } = partitionTeamMembers([
      member("orch", { role: "Manager", role_profile: "orchestrator" }),
      member("worker", { role: "Worker", parent_tab_id: "orch" }),
      member("shell", { spawned_by_tab_id: "worker" }),
      surface("board-worker", "worker"),
      surface("board-orch", "orch"),
      surface("board-orphan", "gone"),
    ]);
    expect(ids(managers)).toEqual(["orch"]);
    expect(ids(workers)).toEqual(["worker"]);
    expect(ids(surfacesByOwner.get("worker") ?? [])).toEqual(["board-worker"]);
    expect(ids(surfacesByOwner.get("orch") ?? [])).toEqual(["board-orch"]);
    expect(ids(terminals)).toEqual(["shell", "board-orphan"]);
  });

  it("groups roleless chat and PTY agents by backing without treating shells as agents", () => {
    const chat = member("chat", {
      backing: { kind: "chat_backend", plugin_id: "provider", session_id: "session" },
      parent_tab_id: "orch",
    }, 4);
    const pty = member("pty", { backing: { kind: "pty" }, agent_type: "provider" }, 7);
    const legacyPty = member("legacy", { agent_type: "another-provider" });
    const { agents, workers, terminals, surfacesByOwner } = partitionTeamMembers([
      chat, pty, legacyPty,
      member("shell", { backing: { kind: "pty" }, parent_tab_id: "orch", spawned_by_tab_id: "orch" }),
      surface("board-chat", "chat"),
      surface("board-pty", "pty"),
    ]);
    expect(agents).toEqual([chat, pty, legacyPty]);
    expect(workers).toEqual([]);
    expect(ids(terminals)).toEqual(["shell"]);
    expect(ids(surfacesByOwner.get("chat") ?? [])).toEqual(["board-chat"]);
    expect(ids(surfacesByOwner.get("pty") ?? [])).toEqual(["board-pty"]);
  });

  it("does not classify plugin surfaces as agents from a stale provider field", () => {
    const { agents, terminals } = partitionTeamMembers([
      { ...surface("board", null), tab: { ...surface("board", null).tab, agent_type: "provider" } },
    ]);
    expect(agents).toEqual([]);
    expect(ids(terminals)).toEqual(["board"]);
  });
});
