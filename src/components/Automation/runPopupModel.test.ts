import assert from "node:assert/strict";
import { fixtureGraph } from "./fixtures.testdata";
import { advanceRunPopups, emptyPopupState } from "./runPopupModel";
import { graphRuns } from "./blueprintRunModel";
import { runPathSteps } from "./runPathModel";
import type { AutomationRun } from "./types";

function graph(id: string, state: AutomationRun["state"] = "completed") {
  const value = fixtureGraph();
  const root = value.nodes[0];
  value.nodes = [{ ...root, history: [{ id, bucket: "branch-b", accepted_at: 10, causal_run: { id, root_node: root.identity.id, root_transition: id, started_at: 10, parent: null }, handlers: [{ handler_id: "handler-c", status: "completed", accepted_at: 11, execution_id: id }] }] }, ...value.nodes.slice(1)];
  value.runs = [{ id, root_node: root.identity.id, root_transition: id, started_at: 10, state, incomplete: false, transitions: [{ node: root.identity, transition_id: id, parent: null }] }];
  return value;
}

const baseline = advanceRunPopups(emptyPopupState(), graph("old"), 0, new Set());
assert.equal(baseline.toasts.length, 0);
const started = advanceRunPopups(baseline.state, graph("first", "running"), 100, new Set());
assert.equal(started.toasts.length, 1);
assert.ok(started.toasts[0].title.endsWith("running"));
const toastId = started.toasts[0].id;
assert.equal(advanceRunPopups(started.state, graph("first"), 6_000, new Set()).toasts.length, 1);
const finished = advanceRunPopups(started.state, graph("first"), 200, new Set([toastId]));
assert.equal(finished.toasts[0].id, toastId);
assert.equal(finished.toasts[0].blueprint_run?.count, 1);
const repeated = advanceRunPopups(finished.state, graph("second"), 300, new Set([toastId]));
assert.equal(repeated.toasts.length, 1);
assert.equal(repeated.toasts[0].blueprint_run?.count, 2);
assert.equal(repeated.toasts[0].blueprint_run?.run.key, "second");
const dismissed = advanceRunPopups(repeated.state, graph("third"), 400, new Set());
assert.equal(dismissed.toasts.length, 0);
const nextWindow = advanceRunPopups(dismissed.state, graph("fourth"), 60_101, new Set());
assert.equal(nextWindow.toasts.length, 1);
assert.equal(nextWindow.toasts[0].blueprint_run?.count, 1);
assert.equal(advanceRunPopups(nextWindow.state, graph("fourth"), 61_000, new Set([toastId])).toasts.length, 0);
assert.deepEqual(runPathSteps(graphRuns(graph("path"))[0]).map((step) => step.name), ["Watcher A", "Worker A"]);
const noopRun = graphRuns(graph("noop"))[0];
noopRun.events[0].edgeId = null;
assert.deepEqual(runPathSteps(noopRun).map((step) => step.name), ["Watcher A"]);
console.log("Run popups skip history, coalesce visible runs and suppress dismissed repeats");
