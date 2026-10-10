import assert from "node:assert/strict";
import { attachment, fixtureGraph, FixtureTransport } from "../Automation/fixtures.testdata";
import { blueprintJobs, mergeJobs, NO_JOBS_FILTER } from "./jobsModel";
import { deleteMergedJob, stopMergedJob, type JobActionContext } from "./jobActions";

async function main() {
  const adapter = new FixtureTransport();
  const graph = fixtureGraph();
  const blueprint = { id: "pipeline-only", name: "Pipeline", owner: { kind: "team" as const, team_id: attachment.team_id, tab_id: "owner-a" }, state: "running" as const, run_id: null, current_step: null, last_event_at_ms: null };
  const standalone = mergeJobs(NO_JOBS_FILTER, [], [], blueprintJobs({ ...graph, blueprints: [blueprint] }))[0];
  let refreshes = 0;
  const context: JobActionContext = {
    adapter,
    jobActions: { stopCron: async () => assert.fail("not a Cron"), deleteCron: async () => assert.fail("not a Cron") },
    graphFor: () => ({ attachment, graph, loading: false, failed: false, stopping: null, stopFailed: null, stop: async () => {}, refresh: async () => { refreshes++; } }),
    refreshCrons: async () => assert.fail("not a Cron"),
  };
  adapter.stopResult.resolve();
  await stopMergedJob(standalone, context);
  assert.deepEqual(adapter.stops, [{ kind: "blueprint", id: "pipeline-only", workspace_id: "workspace-a", team_id: "team-a", owner_tab_id: "owner-a" }]);
  assert.equal(refreshes, 1, "stopping refreshes the graph state");
  await deleteMergedJob(standalone, context);
  assert.deepEqual(adapter.deletes, adapter.stops, "deleting uses the same scoped pipeline identity");

  const companion = mergeJobs(NO_JOBS_FILTER, [], [], blueprintJobs({ ...graph, blueprints: [{ ...blueprint, id: graph.scenarios[0].id }] }))[0];
  await deleteMergedJob(companion, context);
  assert.deepEqual(adapter.deletes[1], graph.scenarios[0].root, "a pipeline with a scenario uses its actual root identity");
  await assert.rejects(deleteMergedJob(standalone, { ...context, graphFor: () => undefined }));
  console.log("Pipeline stop and delete action cases passed");
}

void main().catch((error) => { console.error(error); process.exitCode = 1; });
