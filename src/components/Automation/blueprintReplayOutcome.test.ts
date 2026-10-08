import assert from "node:assert/strict";
import type { GraphRun, RunEvent } from "../../lib/blueprintRun";
import { blueprintModel, nodeKeys } from "./blueprintModel";
import { typedGraph, watchIdentity } from "./blueprintModel.testdata";
import { blueprintReplayOutcome } from "./blueprintReplayOutcome";
import { replayFrame, replaySteps } from "./blueprintRunModel";
import { automationKey } from "./types";

const stage = automationKey(watchIdentity);
const keys = nodeKeys(stage);
const graph = typedGraph();
graph.nodes = graph.nodes.map((node, index) => index ? node : {
  ...node,
  outcomes: node.outcomes.map((outcome) => outcome.bucket === "red" ? { ...outcome, bucket: "error" } : outcome),
  history: [{ id: "selected-transition", bucket: "error", accepted_at: 100, handlers: [
    { handler_id: "notify-owner", status: "completed", execution_id: "delivery", accepted_at: 101, summary: "Telegram check failed" },
  ] }],
});
const model = blueprintModel(graph);
const event: RunEvent = { at: 101, node: "Build watch", outcome: "error", target: "Notify owner", status: "completed", duration: null, edgeId: null, stage, transition: "selected-transition", handler: "notify-owner", reason: "fallback reason" };
const run: GraphRun = { key: "selected-run", at: 100, name: "Build watch", events: [event], state: "completed", incomplete: false };
const error = blueprintReplayOutcome(model, run);
assert.ok(error);
assert.equal(error.nodeKey, keys.action("error", "notify-owner"));
assert.equal(error.failed, true);
assert.equal(error.reason, "Telegram check failed");
assert.equal(error.label, "Stopped at Notify owner · on error: Telegram check failed");
const failedBucketGraph = typedGraph();
failedBucketGraph.nodes = failedBucketGraph.nodes.map((node, index) => index ? node : {
  ...node, outcomes: node.outcomes.map((outcome) => outcome.bucket === "red" ? { ...outcome, bucket: "failed" } : outcome),
});
const failedBucket = blueprintReplayOutcome(blueprintModel(failedBucketGraph), { ...run, events: [{ ...event, outcome: "failed", reason: "failed bucket fixture" }] });
assert.equal(failedBucket?.failed, true);
assert.equal(failedBucket?.nodeKey, keys.action("failed", "notify-owner"));

for (const status of ["failed", "handler_failed", "delivery_failed", "nonzero"]) {
  const failed = blueprintReplayOutcome(model, { ...run, events: [{ ...event, outcome: "green", handler: null, status, transition: "other", reason: `reason for ${status}` }] });
  assert.equal(failed?.failed, true);
  assert.equal(failed?.reason, `reason for ${status}`);
  assert.equal(failed?.nodeKey, keys.decision);
}
const idle: GraphRun = { ...run, events: [{ ...event, outcome: "idle", handler: null, status: "No action", reason: null }] };
const noAction = blueprintReplayOutcome(model, idle);
assert.equal(noAction?.nodeKey, keys.decision);
assert.equal(noAction?.failed, false);
assert.equal(noAction?.label, "Finished: idle, no action");
const steps = replaySteps(model, idle);
assert.deepEqual(steps[steps.length - 1].nodes, [keys.decision]);
const final = replayFrame(steps, 10_000, false);
assert.deepEqual(replayFrame(steps, 20_000, false), final, "the final lit path holds after the clock ends");
assert.equal(final.done, true);
assert.ok(final.nodes.has(keys.decision));

const successModel = blueprintModel(typedGraph());
const success = blueprintReplayOutcome(successModel, { ...run, events: [{ ...event, outcome: "red", status: "completed", transition: "other" }] });
assert.equal(success?.failed, false);
assert.equal(success?.label, "Finished at Notify owner · on red");
const commandGraph = typedGraph();
commandGraph.nodes = commandGraph.nodes.map((node, index) => index ? node : { ...node, history: [{
  id: "command-transition", bucket: "red", accepted_at: 100, handlers: [{
    handler_id: "rebuild", status: "completed", execution_id: "command", accepted_at: 101, summary: "compiler fixture failed",
    command_result: { status: "nonzero", exit_code: 7, resources_confirmed: true, stdout: null, stderr: null, started_at: 100, finished_at: 101 },
  }],
}] });
const command = blueprintReplayOutcome(blueprintModel(commandGraph), { ...run, events: [{ ...event, outcome: "red", handler: "rebuild", transition: "command-transition" }] });
assert.equal(command?.failed, true, "a failed command result stops a completed handler on a non-error bucket");
assert.equal(command?.nodeKey, keys.action("red", "rebuild"));
assert.equal(command?.reason, "compiler fixture failed");
console.log("Replay outcomes identify failure routes and recorded summaries, and hold no-action decision endpoints");
