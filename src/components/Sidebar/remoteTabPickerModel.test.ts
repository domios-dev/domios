import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { groupRemoteTabs, peerTopologyOf, remoteTabKindLabel } from "./remoteTabPickerModel";

const tab = (id: string, fields: Partial<Tab> = {}): Tab => ({ id, pty_id: `pty-${id}`, tab_type: "Terminal", title: id, cwd: null, ...fields });

const topology = peerTopologyOf({
  workspaces: [
    { id: "ws-b", name: "Research", sort_order: 1, tabs: ["planner", "orchestrator", "notes"] },
    { id: "ws-a", name: "Build", sort_order: 0, tabs: ["shell"] },
  ],
  teams: [{ id: "team-r", name: "Heat pumps", workspace_id: "ws-b" }],
  tabs: {},
});

describe("remote Tab picker model", () => {
  it("labels rows by role, then provider, then shell — never by backing kind", () => {
    expect(remoteTabKindLabel(tab("o", { role: "Manager", role_profile: "orchestrator", agent_type: "claude" }))).toBe("Orchestrator");
    expect(remoteTabKindLabel(tab("p", { role: "Manager", role_profile: "planner", agent_type: "codex" }))).toBe("Planner");
    expect(remoteTabKindLabel(tab("w", { role: "Worker", role_profile: null, agent_type: "codex" }))).toBe("Worker");
    expect(remoteTabKindLabel(tab("a", { agent_type: "codex" }))).toBe("Agent: Codex");
    expect(remoteTabKindLabel(tab("c", { pty_id: null, backing: { kind: "chat_backend", plugin_id: "local-model", session_id: "s" } }))).toBe("Agent: local-model");
    expect(remoteTabKindLabel(tab("s", { backing: { kind: "pty" } }))).toBe("Shell");
  });

  it("groups by the peer's workspace in its order, then its Team, keeping its tab order", () => {
    const groups = groupRemoteTabs([
      tab("notes", { workspace_id: "ws-b" }),
      tab("orchestrator", { workspace_id: "ws-b", team_id: "team-r", role: "Manager", role_profile: "orchestrator" }),
      tab("planner", { workspace_id: "ws-b", team_id: "team-r", role: "Manager", role_profile: "planner" }),
      tab("shell", { workspace_id: "ws-a" }),
      tab("hidden", { workspace_id: "ws-a", hidden: true }),
      tab("loose"),
    ], topology);
    expect(groups.map((group) => [group.name, group.teams.map((team) => [team.name, team.rows.map((row) => `${row.title}:${row.kind}`)])])).toEqual([
      ["Build", [[null, ["shell:Shell"]]]],
      ["Research", [[null, ["notes:Shell"]], ["Heat pumps", ["planner:Planner", "orchestrator:Orchestrator"]]]],
      ["Other Tabs", [[null, ["loose:Shell"]]]],
    ]);
  });

  it("still groups when the peer's names are unavailable", () => {
    expect(peerTopologyOf("refused")).toBeNull();
    const groups = groupRemoteTabs([tab("x", { workspace_id: "ws-z", team_id: "team-z" })], null);
    expect(groups).toMatchObject([{ id: "ws-z", name: "Workspace", teams: [{ id: "team-z", name: "Team" }] }]);
  });
});

finish();
