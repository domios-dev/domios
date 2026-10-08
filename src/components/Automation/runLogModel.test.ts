import assert from "node:assert/strict";
import { decisionGraph, fixtureGraph } from "./fixtures.testdata";
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
assert.deepEqual(inspector.properties.map(([key]) => key), ["Type", "Decides with"]);
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

const linked = decisionGraph();
const [rootNode, childNode] = linked.nodes;
const context = { id: "run-one", root_node: rootNode.identity.id, root_transition: "root-one", started_at: 100, parent: null };
const parent = { node: rootNode.identity.id, transition: "root-one", handler: "activate-d" };
linked.nodes = linked.nodes.map((node, index) => index === 0 ? { ...node, history: [
  { id: "root-two", bucket: "passed-d", accepted_at: 100, causal_run: { ...context, id: "run-two", root_transition: "root-two" }, handlers: [{ handler_id: "activate-d", status: "noop", execution_id: null, accepted_at: 100 }] },
  { id: "root-one", bucket: "passed-d", accepted_at: 100, causal_run: context, handlers: [{ handler_id: "activate-d", status: "completed", execution_id: "activation", accepted_at: 100 }] },
] } : index === 1 ? { ...node, history: [
  { id: "child-one", bucket: "passed-e", accepted_at: 1, causal_run: { ...context, parent }, handlers: [{ handler_id: "send-e", status: "completed", execution_id: "send", accepted_at: 1 }] },
] } : node);
linked.runs = [
  { id: "run-one", root_node: context.root_node, root_transition: context.root_transition, started_at: 100, state: "completed", incomplete: false, transitions: [
    { node: childNode.identity, transition_id: "child-one", parent },
    { node: rootNode.identity, transition_id: "root-one", parent: null },
  ] },
  { id: "run-two", root_node: context.root_node, root_transition: "root-two", started_at: 100, state: "completed", incomplete: false, transitions: [{ node: rootNode.identity, transition_id: "root-two", parent: null }] },
];
const linkedRuns = graphRuns(linked);
assert.equal(linkedRuns.length, 2);
assert.deepEqual(linkedRuns[0].events.map((event) => event.node), [rootNode.name, childNode.name]);
assert.deepEqual(linkedRuns[0].events.map((event) => event.at), [100, 1]);
assert.equal(replayEdges(linkedRuns[0]).length, 2);
assert.deepEqual(replayEdges(linkedRuns[1]), []);
assert.equal(graphRuns({ ...linked, nodes: [childNode], runs: linked.runs }).length, 0);
const bounded = graphRuns({ ...linked, nodes: [linked.nodes[0]], runs: linked.runs });
assert.equal(bounded[0].incomplete, true);
