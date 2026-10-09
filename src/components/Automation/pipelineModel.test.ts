import assert from "node:assert/strict";
import { pipelineModel } from "./pipelineModel";
import type { BlueprintSnapshot, BlueprintStepState } from "./types";

function blueprint(overrides: Partial<BlueprintSnapshot> = {}): BlueprintSnapshot {
  return {
    id: "blueprint-a",
    name: "Echo pipeline",
    owner: { kind: "tab", tab_id: "tab-a", team_id: null },
    state: "running",
    run_id: "run-a",
    current_step: 1,
    last_event_at_ms: 1000,
    trigger: { type: "event", summary: "Message received" },
    steps: [
      { index: 0, name: "Echo", kind: "command", title: "Echo command", state: "done", outcome: "success", started_at_ms: 100, ended_at_ms: 200, result_summary: "echoed", reason: null },
      { index: 1, name: "Confirm", kind: "ask", title: "Confirm outcome", state: "running", outcome: null, started_at_ms: 300, ended_at_ms: null, result_summary: null, reason: null },
    ],
    result: { target: { kind: "owner_tab", id: "tab-a" }, state: "delivering", reason: null, body_ref: null },
    ...overrides,
  };
}

const stepStates: BlueprintStepState[] = ["waiting", "running", "done", "failed", "skipped"];
for (const state of stepStates) {
  const view = pipelineModel(blueprint({ steps: [{ ...blueprint().steps![0], state }] }));
  assert.equal(view.nodes[1].state, state, `step state ${state} is preserved`);
}

const branch = pipelineModel(blueprint());
assert.deepEqual(branch.edges.map((edge) => edge.label), [null, "success", null], "step outcomes label their outgoing edges");
assert.equal(branch.nodes.find((node) => node.kind === "step" && node.current)?.key, "step-1", "the current_step index highlights the matching step");
assert.equal(pipelineModel(blueprint({ current_step: null })).nodes.find((node) => node.kind === "step" && node.current)?.key, undefined, "an explicit null current step keeps closed runs idle");

for (const state of ["waiting", "delivering", "delivered", "failed"] as const) {
  const view = pipelineModel(blueprint({ result: { ...blueprint().result!, state } }));
  assert.equal(view.nodes[view.nodes.length - 1]?.state, state, `result state ${state} is preserved`);
}

const optional = pipelineModel({ id: "old", name: "Legacy", state: "idle", run_id: null, current_step: null, last_event_at_ms: null });
assert.equal(optional.owner.kind, "tab");
assert.equal(optional.nodes.length, 2, "snapshots without trigger, steps, owner and result still render a pipeline");
assert.equal(optional.nodes[0].summary, "Trigger details unavailable");
assert.equal(optional.nodes[optional.nodes.length - 1]?.state, "waiting");
assert.equal(optional.nodes[optional.nodes.length - 1]?.targetSummary, "Target unavailable");

console.log("Blueprint pipeline model cases passed");
