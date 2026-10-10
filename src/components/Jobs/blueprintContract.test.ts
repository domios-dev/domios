import assert from "node:assert/strict";
import { pipelineContractGolden } from "../Automation/blueprintContract.testdata";
import { scopedAutomationGraph } from "../Automation/model";
import { blueprintModel } from "../Automation/blueprintModel";
import { blueprintJobs, canStopJob, mergeJobs, NO_JOBS_FILTER, jobsIndicator, tabJobState } from "./jobsModel";

const graph = pipelineContractGolden();
const scoped = scopedAutomationGraph(graph, graph);
const [job] = blueprintJobs(scoped);
assert.equal(job.pipeline.state, "idle");
assert.deepEqual(job.pipeline.nodes.filter((node) => node.kind === "step").map((node) => node.summary), ["emit", "match"]);
assert.ok(job.pipeline.nodes.every((node) => !node.current));
assert.equal(job.pipeline.runs?.[0].id, "run-echo");
assert.equal(job.pipeline.runs?.[0].state, "completed");
assert.equal(job.pipeline.nodes[job.pipeline.nodes.length - 1]?.state, "delivered");
assert.deepEqual(blueprintModel(scoped).nodes, [], "pipelines never produce flat probe or decision nodes");
console.log("Rust pipeline contract reaches Jobs with real steps and retained runs");

const [merged] = mergeJobs(NO_JOBS_FILTER, [], [], [job]);
assert.equal(canStopJob(merged), true, "an idle scheduled pipeline can still be stopped");
assert.equal(tabJobState(merged), "scheduled");
assert.deepEqual(jobsIndicator({ crons: [], sentinels: [], blueprints: [job] }, NO_JOBS_FILTER), { count: 1, running: 0 });
