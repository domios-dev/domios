import assert from "node:assert/strict";
import { successfulSolutionScope } from "./blueprintSolutions";
import { blueprintJobs, mergeJobs, NO_JOBS_FILTER } from "./jobsModel";
import { fixtureGraph, fixtureNode, identity } from "../Automation/fixtures.testdata";
import { blueprintScenarioJobs } from "../Automation/scenarios";

const root = identity("blueprint", "proof");
const graph = { ...fixtureGraph(), scenarios: [{ id: "proof", name: "Watch", root }], nodes: [{ ...fixtureNode(root, "Watch"), state: "stopped", solution_success: false }] };
let jobs = mergeJobs(NO_JOBS_FILTER, [], blueprintScenarioJobs(graph, graph));
assert.equal(successfulSolutionScope(jobs[0]), null);
graph.nodes[0].solution_success = true;
jobs = mergeJobs(NO_JOBS_FILTER, [], blueprintScenarioJobs(graph, graph));
assert.deepEqual(successfulSolutionScope(jobs[0]), { workspace_id: root.workspace_id, team_id: root.team_id, id: root.id });
const snapshot = { id: "pipeline", name: "Pipeline", state: "stopped" as const, run_id: null, current_step: null, last_event_at_ms: null, result: { target: { kind: "notify" as const, id: null }, state: "failed" as const, reason: null, body_ref: null } };
const pipelineJobs = (value: unknown) => mergeJobs(NO_JOBS_FILTER, [], [], blueprintJobs({ ...fixtureGraph(), blueprints: [value as typeof snapshot] }));
assert.equal(successfulSolutionScope(pipelineJobs(snapshot)[0]), null);
assert.ok(successfulSolutionScope(pipelineJobs({ ...snapshot, result: { ...snapshot.result, state: "delivered" } })[0]));
console.log("blueprintSolutions: 4 assertions passed");
