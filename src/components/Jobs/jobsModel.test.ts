import assert from "node:assert/strict";
import type { CronJob } from "../../lib/types";
import { fixtureGraph, fixtureNode, identity } from "../Automation/fixtures.testdata";
import { sentinelJobs } from "../Automation/scenarios";
import { ALL_JOBS, clearableJobs, jobsAccessibleName, jobsForTab, jobsIndicator, mergeJobs, NO_JOBS_FILTER, tabJobAccessibleName } from "./jobsModel";

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
const noTeam = { ...identity("sentinel", "sentinel-no-team"), team_id: null };
const crossTeamGraph = {
  ...graph,
  all_teams: true,
  team_id: null,
  scenarios: [...graph.scenarios, { id: otherTeam.id, name: otherTeam.id, root: otherTeam }, { id: bucketOnly.id, name: bucketOnly.id, root: bucketOnly }, { id: "scenario-no-team", name: "Sentinel no team", root: noTeam }],
  nodes: [...graph.nodes,
    { ...fixtureNode(otherTeam, "question: inspect workspace"), identity: otherTeam, name: otherTeam.id, purpose: "question: inspect workspace", state: "stopped", sentinel_definition: { source: "stored_agent_definition" as const, definition_id: otherTeam.id, creator_tab_id: otherTeam.owner_tab_id! } },
    { ...fixtureNode(bucketOnly, "bucket fallback"), identity: bucketOnly, name: bucketOnly.id, purpose: null, outcomes: [{ bucket: "first-completed", handlers: [] }], state: "stopped", sentinel_definition: { source: "stored_agent_definition" as const, definition_id: bucketOnly.id, creator_tab_id: bucketOnly.owner_tab_id! } },
    { ...fixtureNode(noTeam, "no-team graph"), identity: noTeam, state: "stopped", sentinel_definition: { source: "stored_agent_definition" as const, definition_id: noTeam.id, creator_tab_id: noTeam.owner_tab_id! } },
  ],
};
const sentinelEntries = sentinelJobs(crossTeamGraph, { workspace_id: "workspace-a", team_id: ALL_JOBS });

const all = mergeJobs({ workspaceId: "workspace-a", teamId: null }, [cron("team-cron", "team-a"), cron("no-team-cron", null), cron("foreign-cron", null, "workspace-b")], sentinelEntries);
assert.deepEqual(all.map((job) => job.title).sort(), ["first-completed", "no-team-cron", "question: inspect workspace", "team-cron", "Sentinel no team", "Sentinel scenario A"].sort());
assert.equal(mergeJobs({ workspaceId: "workspace-a", teamId: "team-a" }, [cron("team-cron", "team-a"), cron("no-team-cron", null)], sentinelEntries).length, 2);
assert.deepEqual(mergeJobs({ workspaceId: null, teamId: "team-b" }, [cron("team-cron", "team-a"), cron("foreign-team-cron", "team-b", "workspace-b")], sentinelEntries).map((job) => job.title), ["foreign-team-cron", "question: inspect workspace"]);
assert.equal(mergeJobs({ workspaceId: null, teamId: null }, [cron("team-cron", "team-a"), cron("foreign-cron", null, "workspace-b")], sentinelEntries).length, 2 + sentinelEntries.length, "no capsules shows every job");
assert.equal(clearableJobs(all).filter((job) => job.kind === "sentinel").length, 3, "every stopped graph in the filter is clearable");
const workspaceA = { workspaceId: "workspace-a", teamId: null };
const idleCrons = [cron("team-cron", "team-a"), cron("no-team-cron", null), cron("foreign", null, "workspace-b")];
assert.deepEqual(jobsIndicator({ crons: idleCrons, sentinels: sentinelEntries }, workspaceA), { count: 6, running: 0 }, "enabled idle crons and stopped graphs count but are not running");
assert.deepEqual(jobsIndicator({ crons: idleCrons, sentinels: sentinelEntries }, NO_JOBS_FILTER), { count: 7, running: 0 }, "machine scope counts every workspace");
assert.deepEqual(jobsIndicator({ crons: [], sentinels: [] }, workspaceA), { count: 0, running: 0 });
const activeGraph = { ...crossTeamGraph, nodes: crossTeamGraph.nodes.map((node) => node.identity.id === graph.scenarios[0].root.id ? { ...node, state: "active" } : node) };
assert.deepEqual(jobsIndicator({ crons: idleCrons, sentinels: sentinelJobs(activeGraph, { workspace_id: "workspace-a", team_id: ALL_JOBS }) }, workspaceA), { count: 6, running: 1 }, "a live graph is running");
const runningCron = { ...cron("running-cron", null, "workspace-b", false), last_run: { run_id: "r", started_at: 1, finished_at: null, status: "Running" as const, tab_id: null } };
assert.deepEqual(jobsIndicator({ crons: [...idleCrons, runningCron], sentinels: [] }, workspaceA), { count: 2, running: 0 }, "another workspace's run stays out of scope");
assert.deepEqual(jobsIndicator({ crons: [...idleCrons, runningCron], sentinels: [] }, NO_JOBS_FILTER), { count: 4, running: 1 }, "a cron mid-run is running even when disabled");
assert.equal(jobsAccessibleName({ count: 0, running: 0 }), "Jobs");
assert.equal(jobsAccessibleName({ count: 3, running: 0 }), "Jobs, 3");
assert.equal(jobsAccessibleName({ count: 3, running: 1 }), "Jobs, 3, 1 running");

const created = { ...cron("created-here", null), creator_tab_id: "tab-a" };
const reportsHere = { ...cron("reports-here", null, "workspace-b", false), report_target_tab_id: "tab-a" };
const runsHere = { ...cron("runs-here", null), last_run: { run_id: "r1", started_at: 1, finished_at: null, status: "Running" as const, tab_id: "tab-a" } };
const asksHere = { ...cron("asks-here", null), creator_tab_id: "tab-a", last_run: { run_id: "r2", started_at: 1, finished_at: null, status: "AwaitingOwnerPrompt" as const, tab_id: "run-tab" } };
const unbound = { ...cron("unbound", null), creator_tab_id: "tab-b" };
const tabSentinels = sentinelJobs(graph, { workspace_id: "workspace-a", team_id: "team-a" });
const tabJobs = mergeJobs(NO_JOBS_FILTER, [created, reportsHere, runsHere, asksHere, unbound], tabSentinels);
assert.deepEqual(jobsForTab(tabJobs, "tab-a"), [
  { id: "asks-here", title: "asks-here", kind: "cron", state: "waiting on you" },
  { id: "runs-here", title: "runs-here", kind: "cron", state: "running" },
  { id: "created-here", title: "created-here", kind: "cron", state: "scheduled" },
  { id: "reports-here", title: "reports-here", kind: "cron", state: "stopped" },
], "creator, report target and run tab bind a cron; attention first");
assert.deepEqual(jobsForTab(tabJobs, "owner-a"), [{ id: "scenario-a", title: "Sentinel scenario A", kind: "sentinel", state: "stopped" }], "the Sentinel creator tab is bound; a paused graph is stopped");
assert.deepEqual(jobsForTab(tabJobs, "worker-a").map((job) => job.id), ["scenario-a"], "a tab a Sentinel delivers into is bound");
const liveSentinels = sentinelJobs({ ...graph, nodes: graph.nodes.map((node) => node.identity.kind === "sentinel" ? { ...node, state: "active" } : node) }, { workspace_id: "workspace-a", team_id: "team-a" });
assert.equal(jobsForTab(mergeJobs(NO_JOBS_FILTER, [], liveSentinels), "owner-a")[0].state, "running", "a live graph runs, matching the sidebar count");
assert.deepEqual(jobsForTab(tabJobs, "nobody"), []);
assert.equal(tabJobAccessibleName({ id: "c", title: "rust-rss-new-videos", kind: "cron", state: "running" }), "Job rust-rss-new-videos, running");
console.log("Jobs model and indicator and tab-binding cases passed");
