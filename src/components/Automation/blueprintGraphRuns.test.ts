import assert from "node:assert/strict";
import { decisionGraph, fixtureGraph } from "./fixtures.testdata";
import { graphRuns } from "./blueprintRunModel";
import { blueprintModel, nodeKeys, wireId } from "./blueprintModel";
import { automationKey } from "./types";

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
const edgesOf = (run: ReturnType<typeof graphRuns>[number]) => run.events.flatMap((event) => event.edgeId === null ? [] : [event.edgeId]);
const runs = graphRuns(graph);
const counts = { enabled: true, exchanges: 257, actions: 260, safety_limit: 16384, rate_limit_per_minute: 120 };
const conversationGraph = { ...graph, nodes: graph.nodes.map((node) => ({ ...node, conversation: counts })) };
assert.deepEqual(graphRuns(conversationGraph).map((run) => run.conversation), [counts, counts, counts]);
assert.deepEqual(runs.map((run) => run.at), [30, 20, 10]);
assert.deepEqual(runs[0].events.map((event) => event.at), [31, 32]);
assert.deepEqual(edgesOf(runs[0]), [wireId(nodeKeys(automationKey(graph.nodes[0].identity)).decision, "bucket:branch-b", nodeKeys(automationKey(graph.nodes[0].identity)).action("branch-b", "handler-c"))]);
assert.deepEqual(edgesOf(runs[1]), [wireId(nodeKeys(automationKey(graph.nodes[0].identity)).decision, "bucket:branch-a", nodeKeys(automationKey(graph.nodes[0].identity)).action("branch-a", "handler-a"))]);
assert.equal(runs[0].events[0].target, "Worker A");
assert.equal(runs[2].events[0].edgeId, null);
const amended = { ...graph, nodes: [{ ...graph.nodes[0], history: [{ ...graph.nodes[0].history![1], routes_current: false }] }] };
assert.deepEqual(edgesOf(graphRuns(amended)[0]), []);
for (const run of runs) for (const edge of edgesOf(run)) assert.ok(blueprintModel(graph).wires.some((wire) => wire.id === edge));
console.log("Run ordering and exact route replay passed");

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
assert.equal(edgesOf(linkedRuns[0]).length, 2);
assert.deepEqual(edgesOf(linkedRuns[1]), []);
assert.equal(graphRuns({ ...linked, nodes: [childNode], runs: linked.runs }).length, 0);
const bounded = graphRuns({ ...linked, nodes: [linked.nodes[0]], runs: linked.runs });
assert.equal(bounded[0].incomplete, true);
