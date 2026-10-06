import assert from "node:assert/strict";
import type { Team } from "../../lib/types";
import { addTeamCapsule, addWorkspaceCapsule, jobsEntryFilter, jobsLayout, jobsOpenEntry, removeJobsCapsule } from "./jobsLayout";
import { NO_JOBS_FILTER } from "./jobsModel";

const workspaces = [{ id: "ws-a", name: "Alpha" }, { id: "ws-b", name: "Beta" }];
const team = (id: string, workspace_id: string, extra: Partial<Team> = {}): Team => ({ id, name: `Team ${id}`, lifecycle: "active", created_at: 0, workspace_id, ...extra });
const teams = [team("t-a", "ws-a"), team("t-b", "ws-b"), team("t-remote", "ws-a", { remote_peer_name: "peer" })];

const cases: Array<[string, () => void]> = [
  ["a machine-wide open starts unfiltered and a workspace open keeps its workspace and Team", () => {
    assert.deepEqual(jobsEntryFilter(jobsOpenEntry("ws-general", "machine")), NO_JOBS_FILTER);
    assert.deepEqual(jobsEntryFilter(jobsOpenEntry("ws-general", "machine", "t-a")), NO_JOBS_FILTER);
    assert.deepEqual(jobsEntryFilter(jobsOpenEntry("ws-a", "workspace")), { workspaceId: "ws-a", teamId: null });
    assert.deepEqual(jobsEntryFilter(jobsOpenEntry("ws-a", "workspace", "t-a")), { workspaceId: "ws-a", teamId: "t-a" });
  }],
  ["the machine entry starts with no capsule and every workspace as a source", () => {
    const layout = jobsLayout(jobsEntryFilter({ kind: "machine" }), workspaces, teams);
    assert.deepEqual(layout.capsules, []);
    assert.deepEqual(layout.sources, ["ws-a", "ws-b"]);
    assert.equal(layout.cronWorkspace, null);
    assert.equal(layout.emptyScope, "machine");
    assert.equal(layout.canWiden, false);
  }],
  ["removing the workspace capsule from the workspace entry yields exactly the machine-wide layout", () => {
    const fromWorkspace = removeJobsCapsule(jobsEntryFilter({ kind: "workspace", workspaceId: "ws-a" }), "workspace");
    assert.deepEqual(fromWorkspace, jobsEntryFilter({ kind: "machine" }));
    assert.deepEqual(jobsLayout(fromWorkspace, workspaces, teams), jobsLayout(jobsEntryFilter({ kind: "machine" }), workspaces, teams));
  }],
  ["the same filter gives the same layout whichever entry produced it", () => {
    const viaWorkspace = jobsEntryFilter({ kind: "workspace", workspaceId: "ws-b", teamId: null });
    const viaMachine = addWorkspaceCapsule(jobsEntryFilter({ kind: "machine" }), "ws-b", teams);
    assert.deepEqual(jobsLayout(viaWorkspace, workspaces, teams), jobsLayout(viaMachine, workspaces, teams));
  }],
  ["a workspace capsule narrows sources and Team options to that workspace and hides the workspace add", () => {
    const layout = jobsLayout(jobsEntryFilter({ kind: "workspace", workspaceId: "ws-a" }), workspaces, teams);
    assert.deepEqual(layout.capsules, [{ axis: "workspace", value: "Alpha" }]);
    assert.deepEqual(layout.workspaceOptions, []);
    assert.deepEqual(layout.teamOptions.map((option) => [option.id, option.workspaceName]), [["t-a", null]]);
    assert.deepEqual(layout.sources, ["ws-a"]);
    assert.equal(layout.emptyScope, "workspace");
    assert.equal(layout.canWiden, true);
  }],
  ["without a workspace capsule every local Team is offered with its workspace, remote Teams never", () => {
    const layout = jobsLayout(jobsEntryFilter({ kind: "machine" }), workspaces, teams);
    assert.deepEqual(layout.teamOptions.map((option) => [option.id, option.workspaceName]), [["t-a", "Alpha"], ["t-b", "Beta"]]);
  }],
  ["a Team capsule alone reads its workspace and an unknown Team is unavailable but removable", () => {
    const layout = jobsLayout(addTeamCapsule(jobsEntryFilter({ kind: "machine" }), "t-b"), workspaces, teams);
    assert.deepEqual(layout.sources, ["ws-b"]);
    assert.equal(layout.emptyScope, "team");
    assert.deepEqual(layout.teamOptions, []);
    const stale = jobsLayout({ workspaceId: null, teamId: "t-remote" }, workspaces, teams);
    assert.equal(stale.teamAvailable, false);
    assert.deepEqual(stale.capsules.map((capsule) => capsule.axis), ["team"]);
  }],
  ["choosing another workspace drops a Team of a different workspace and keeps a matching one", () => {
    assert.deepEqual(addWorkspaceCapsule({ workspaceId: null, teamId: "t-a" }, "ws-b", teams), { workspaceId: "ws-b", teamId: null });
    assert.deepEqual(addWorkspaceCapsule({ workspaceId: null, teamId: "t-a" }, "ws-a", teams), { workspaceId: "ws-a", teamId: "t-a" });
  }],
];

for (const [, run] of cases) run();
console.log(`${cases.length} Jobs layout cases passed`);
