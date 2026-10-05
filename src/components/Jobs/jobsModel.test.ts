import assert from "node:assert/strict";
import type { CronJob } from "../../lib/types";
import { fixtureGraph, fixtureNode, identity } from "../Automation/fixtures.testdata";
import { sentinelJobs } from "../Automation/scenarios";
import { ALL_JOBS, clearableJobs, mergeJobs } from "./jobsModel";

function cron(id: string, teamId: string | null, workspaceId = "workspace-a", enabled = true): CronJob {
  return {
    id, name: id, description: null, workspace_id: workspaceId, team_id: teamId, schedule: "*/5 * * * *", enabled,
    agent: "fixture", model: null, reasoning_effort: null, task: id, cwd: null, role: null, report_target_tab_id: null,
    oneshot: false, max_history: 10, created_at: 1, last_run: null, next_run: null, jitter_max_ms: null,
  };
}

const graph = fixtureGraph();
const otherTeam = { ...identity("sentinel", "sentinel-other-team"), team_id: "team-b" };
const bucketOnly = { ...identity("sentinel", "sentinel-bucket-only"), team_id: "team-c" };
const crossTeamGraph = {
  ...graph,
  all_teams: true,
  team_id: null,
  scenarios: [...graph.scenarios, { id: otherTeam.id, name: otherTeam.id, root: otherTeam }, { id: bucketOnly.id, name: bucketOnly.id, root: bucketOnly }],
  nodes: [...graph.nodes,
    { ...fixtureNode(otherTeam, "question: inspect workspace"), identity: otherTeam, name: otherTeam.id, purpose: "question: inspect workspace", state: "stopped", sentinel_definition: { source: "stored_agent_definition" as const, definition_id: otherTeam.id, creator_tab_id: otherTeam.owner_tab_id! } },
    { ...fixtureNode(bucketOnly, "bucket fallback"), identity: bucketOnly, name: bucketOnly.id, purpose: null, outcomes: [{ bucket: "first-completed", handlers: [] }], state: "stopped", sentinel_definition: { source: "stored_agent_definition" as const, definition_id: bucketOnly.id, creator_tab_id: bucketOnly.owner_tab_id! } },
  ],
};
const sentinelEntries = sentinelJobs(crossTeamGraph, { workspace_id: "workspace-a", team_id: ALL_JOBS });

const all = mergeJobs("workspace-a", ALL_JOBS, [cron("team-cron", "team-a"), cron("no-team-cron", null), cron("foreign-cron", null, "workspace-b")], sentinelEntries);
assert.deepEqual(all.map((job) => job.title).sort(), ["first-completed", "no-team-cron", "question: inspect workspace", "team-cron", "Sentinel scenario A"].sort());
assert.equal(mergeJobs("workspace-a", "team-a", [cron("team-cron", "team-a"), cron("no-team-cron", null)], sentinelEntries).length, 2);
assert.deepEqual(mergeJobs("workspace-a", null, [cron("team-cron", "team-a"), cron("no-team-cron", null)], sentinelEntries).map((job) => job.title), ["no-team-cron"]);
assert.equal(clearableJobs(all).filter((job) => job.kind === "sentinel").length, 2);
console.log("4 Jobs model cases passed");
