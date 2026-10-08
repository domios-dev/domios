import assert from "node:assert/strict";
import { fixtureGraph } from "./fixtures.testdata";
import { graphRuns, replayEdges, replayFrame } from "./runLogModel";
import { nodeInspector } from "./nodeInspector";
import { projectAutomationGraph } from "./projection";

const graph = fixtureGraph();
graph.nodes = graph.nodes.map((node, index) => index ? node : {
  ...node, history: [
    { id: "old", bucket: "branch-a", accepted_at: 10, handlers: [] },
    { id: "new", bucket: "branch-b", accepted_at: 30, handlers: [
      { handler_id: "handler-c", status: "completed", accepted_at: 31, execution_id: "e1" },
      { handler_id: "unknown", status: "skipped", accepted_at: 32, execution_id: null },
    ] },
    { id: "middle", bucket: "branch-a", accepted_at: 20, handlers: [
      { handler_id: "handler-a", status: "failed", accepted_at: 21, execution_id: "e2" },
    ] },
  ],
});
const runs = graphRuns(graph);
assert.deepEqual(runs.map((run) => run.at), [30, 20, 10]);
assert.deepEqual(runs[0].events.map((event) => event.at), [31, 32]);
assert.deepEqual(replayEdges(runs[0]), ["edge-b"]);
assert.deepEqual(replayEdges(runs[1]), ["edge-a"]);
assert.equal(runs[0].events[0].target, "Worker A");
assert.equal(runs[2].events[0].edgeId, null);
const amended = { ...graph, nodes: [{ ...graph.nodes[0], history: [{ ...graph.nodes[0].history![1], routes_current: false }] }] };
assert.deepEqual(replayEdges(graphRuns(amended)[0]), []);
assert.equal(replayFrame(["first", "second"], 0, false).active, "first");
assert.equal(replayFrame(["first", "second"], 850, false).active, "second");
assert.equal(replayFrame(["first", "second"], 1700, false).active, null);
assert.deepEqual(replayFrame(["first", "second"], 450, true), { highlighted: ["first", "second"], active: null, progress: 0 });
const node = graph.nodes[0];
const inspector = nodeInspector(node, null);
assert.deepEqual(inspector.properties.map(([key]) => key), ["Kind", "Routes"]);
assert.equal(inspector.code, null);
const handler = { id: "build", type: "command", target: null, availability: "available" as const, label: "Build", timeout_ms: 1000, last_result: null };
const coded = { ...node, outcomes: [{ bucket: "ready", handlers: [handler] }], detail: { definition: { probe: "printf ready", commands: [{ bucket: "ready", handler_id: "build", label: "Build", command: "make", command_truncated: false, timeout_ms: 1000 }] } } };
const projected = projectAutomationGraph({ ...graph, edges: [], nodes: [coded] });
const action = projected.nodes.find((entry) => entry.action)?.action;
assert.ok(action);
assert.equal(nodeInspector(coded, action).name, "Build");
assert.equal(nodeInspector(coded, action).code?.text, "make");
assert.equal(nodeInspector(coded, null).code?.text, "printf ready");
assert.equal(nodeInspector(coded, { ...action, bucket: "other" }).code, null);
console.log("Run ordering, exact route replay and node-only inspector model passed");
