import assert from "node:assert/strict";
import { teamCloseResourcesForWorkspace } from "./teamCloseResources";
import type { BlueprintSnapshot } from "../Automation/types";
import type { Tab, Team, Workspace } from "../../lib/types";

const workspace = { id: "workspace-a", name: "Workspace A", tabs: ["member-a", "other-tab"], active_tab_id: null, cwd: null } satisfies Workspace;
const tabs = {
  "member-a": { id: "member-a", title: "Team member", team_id: "team-a" } as Tab,
  "other-tab": { id: "other-tab", title: "Other tab", team_id: null } as Tab,
};
const teams: Team[] = [{ id: "team-a", name: "Team A", lifecycle: "active", created_at: 0, workspace_id: workspace.id }];
const blueprints: BlueprintSnapshot[] = [
  { id: "team-run", name: "Team pipeline", owner: { kind: "team", tab_id: "member-a", team_id: "team-a" }, state: "running", run_id: "run-1", current_step: 0, last_event_at_ms: 1 },
  { id: "member-run", name: "Member pipeline", owner: { kind: "tab", tab_id: "member-a", team_id: null }, state: "running", run_id: "run-2", current_step: 0, last_event_at_ms: 2 },
  { id: "other-run", name: "Other pipeline", owner: { kind: "tab", tab_id: "other-tab", team_id: null }, state: "running", run_id: "run-3", current_step: 0, last_event_at_ms: 3 },
  { id: "idle", name: "Idle pipeline", owner: { kind: "team", tab_id: "member-a", team_id: "team-a" }, state: "idle", run_id: null, current_step: null, last_event_at_ms: null },
];

const [team] = teamCloseResourcesForWorkspace(workspace, tabs, teams, [], [], blueprints);
assert.deepEqual(team.blueprints, ["Team pipeline", "Member pipeline"]);

console.log("Team close Blueprint inventory cases passed");
