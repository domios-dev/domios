import assert from "node:assert/strict";
import type { WorkspaceClosePreview } from "../../../gen/WorkspaceClosePreview";
import type { TransportClient } from "../../lib/transportClient";
import { readTeamClosePlanMetadata } from "./teamClosePlanRead";

async function main() {
  const preview: WorkspaceClosePreview = {
    installation_id: "local",
    workspace_id: "workspace/a",
    workspace_name: "Workspace A",
    protected: false,
    agent_tabs: [],
    remote_members: [],
    crons: [],
    blueprints: [],
    warnings: [],
    teams: [{
      id: "team/a",
      name: "Team A",
      remote: false,
      members: [{ tab_id: "member-a", title: "Team member", role: "Worker", role_profile: null, remote: false }],
      crons: ["Team schedule"],
      blueprints: ["Team pipeline", "Member pipeline"],
      tasks: [{ plan_id: "foreign-plan-with-team-attempt", plan_title: "Shared plan", task_id: "a",
        task_name: "Member task", status: "in_progress", orphan: false, remote_attempt: false }],
      open_attempts: [],
      lanes: [],
    }],
  };
  const requests: string[] = [];
  const controller = new AbortController();
  const transport = {
    async localJson<T>(method: string, path: string, body?: unknown, options?: { signal?: AbortSignal }) {
      assert.equal(method, "GET");
      assert.equal(body, undefined);
      assert.equal(options?.signal, controller.signal);
      requests.push(path);
      return preview as T;
    },
  } satisfies Pick<TransportClient, "localJson">;
  const [team] = (await readTeamClosePlanMetadata("workspace/a", "team/a", controller.signal, transport)).teams;
  assert.deepEqual(requests, ["/api/workspaces/workspace%2Fa/close-preview?resources_only=true&team_id=team%2Fa"]);
  assert.deepEqual(team.members.map(member => member.tabId), ["member-a"]);
  assert.deepEqual(team.crons, ["Team schedule"]);
  assert.deepEqual(team.blueprints, ["Team pipeline", "Member pipeline"]);
  assert.deepEqual(team.lanes, ["Shared plan: Member task"]);
  await assert.rejects(readTeamClosePlanMetadata("workspace/a", "other-team", controller.signal, transport), /selected scope/);
  await readTeamClosePlanMetadata("workspace/a", null, controller.signal, transport);
  assert.equal(requests.at(-1), "/api/workspaces/workspace%2Fa/close-preview?resources_only=true");
}

void main();
