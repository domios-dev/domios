import { describe, expect, it } from "vitest";
import type { CronJob } from "../../lib/types";
import { call, document, journal } from "../Automation/blueprintViewer.testdata";
import { fixtureGraph } from "../Automation/fixtures.testdata";
import { blueprintScenarioJobs } from "../Automation/scenarios";
import { ALL_JOBS_SCOPE, type AutomationGraph, type BlueprintSnapshot } from "../Automation/types";
import { blueprintJobs, isJobRunning, jobsIndicator, mergeJobs, mergedJobId, NO_JOBS_FILTER } from "./jobsModel";

const base = fixtureGraph();
const snapshot: BlueprintSnapshot = {
  id: base.scenarios[0].id, name: "Retained pipeline", state: "stopped",
  owner: { kind: "team", tab_id: "owner-a", team_id: "team-a" },
  run_id: null, current_step: null, last_event_at_ms: 1000,
  stopped_at: 1000, deletes_at: 2000, call_graph: document, expanded_calls: [call],
  runs: [{ id: "past", state: "failed", closed: true, started_at_ms: 500, last_event_at_ms: 800,
    current_step: null, steps: [], call_graph: document, expanded_calls: [call],
    nested_run: { id: "past", nodes: journal },
    result: { target: { kind: "owner_tab", id: "owner-a" }, state: "failed", reason: "failed", body_ref: null } }],
};
const graph: AutomationGraph = {
  ...base, all_teams: true, team_id: null,
  blueprints: [snapshot,
    { ...snapshot, id: "watch-snapshot", call_graph: { ...document, kind: "watch" } },
    { ...snapshot, id: "other-team", owner: { kind: "tab", tab_id: "owner-b", team_id: "team-b" }, state: "running" }],
  quarantined: [{ id: "invalid", name: "Invalid", state: "quarantined", code: "invalid", reason: "Invalid definition" }],
};
const cron: CronJob = {
  id: "cron", name: "Cron", description: null, workspace_id: "workspace-a", team_id: "team-a",
  schedule: "*/5 * * * *", enabled: false, agent: "fixture", model: null, reasoning_effort: null,
  task: "fixture", cwd: null, role: null, report_target_tab_id: null, oneshot: false,
  max_history: 10, created_at: 1, last_run: null, next_run: null, jitter_max_ms: null,
};

describe("scoped Jobs counts", () => {
  const sentinels = blueprintScenarioJobs(graph, { workspace_id: graph.workspace_id, team_id: ALL_JOBS_SCOPE });
  const blueprints = blueprintJobs(graph);
  const crons = [cron, { ...cron, id: "foreign-cron", workspace_id: "workspace-b" }];

  it("counts stored roots once, retaining stopped jobs without counting calls, runs or quarantine", () => {
    const scope = { workspaceId: "workspace-a", teamId: "team-a" };
    const listed = mergeJobs(scope, crons, sentinels, blueprints);
    expect(listed.map(mergedJobId).sort()).toEqual(["cron", "scenario-a"]);
    expect(jobsIndicator({ crons, sentinels, blueprints }, scope)).toEqual({ count: 2, running: 0 });
  });

  for (const scope of [NO_JOBS_FILTER, { workspaceId: "workspace-a", teamId: null }, { workspaceId: "workspace-a", teamId: "team-a" }, { workspaceId: "workspace-a", teamId: "team-b" }]) {
    it(`uses the list definition for ${JSON.stringify(scope)}`, () => {
      const listed = mergeJobs(scope, crons, sentinels, blueprints);
      const teamRows = mergeJobs(scope, [], sentinels, blueprints);
      const cronRows = mergeJobs(scope, crons, []);
      expect(teamRows.length + cronRows.length).toBe(listed.length);
      expect(teamRows.filter(isJobRunning).length + cronRows.filter(isJobRunning).length).toBe(listed.filter(isJobRunning).length);
      expect(jobsIndicator({ crons, sentinels, blueprints }, scope).count).toBe(listed.length);
    });
  }
});
