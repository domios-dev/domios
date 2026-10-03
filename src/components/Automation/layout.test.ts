import assert from "node:assert/strict";
import { boxesOverlap } from "../Canvas/layout";
import { layoutAutomationGraph } from "./layout";
import { scopedAutomationGraph } from "./model";
import { attachment, fixtureGraph, fixtureNode, identity, multiScenarioGraph } from "./fixtures.testdata";
import { automationKey, type AutomationEdge, type AutomationGraph, type AutomationIdentity } from "./types";

const tests: Array<[string, () => void]> = [];
function test(name: string, run: () => void) { tests.push([name, run]); }

const definition = (target: AutomationIdentity) => ({ source: "stored_agent_definition" as const, definition_id: target.id, creator_tab_id: target.owner_tab_id! });
function route(id: string, source: AutomationIdentity, target: AutomationIdentity, kind: "configured" | "observed" = "configured"): AutomationEdge {
  return { id, source, target, bucket: `bucket-${id}`, handler_id: `handler-${id}`, handler_type: "sentinel", result: null, evidence: kind === "configured" ? { kind, route_id: `route-${id}` } : { kind, execution_id: `execution-${id}` } };
}

/** Root branches to two stages that join on one shared target, plus a skip edge and a malformed cycle back to the root. */
function branchedGraph(): AutomationGraph {
  const [root, left, right, join] = ["root", "left", "right", "join"].map((id) => identity("sentinel", id));
  const nodes = [root, left, right, join].map((target) => ({ ...fixtureNode(target, `name-${target.id}`), sentinel_definition: definition(target) }));
  return {
    workspace_id: attachment.workspace_id, team_id: attachment.team_id, revision: "layout",
    scenarios: [{ id: "scenario-layout", name: "scenario-layout", root }],
    nodes,
    edges: [route("a", root, left), route("b", root, right), route("c", left, join, "observed"), route("d", right, join), route("e", root, join), route("f", join, root), route("g", left, left)],
  };
}

function scoped(graph: AutomationGraph) {
  return scopedAutomationGraph(graph, attachment);
}

function boxOf(layout: ReturnType<typeof layoutAutomationGraph>, target: AutomationIdentity) {
  const node = layout.byId.get(automationKey(target));
  assert.ok(node, `${target.id} laid out`);
  return node.box;
}

test("edges flow left to right from the scenario root and a shared join sits after both branches", () => {
  const graph = scoped(branchedGraph());
  const layout = layoutAutomationGraph(graph);
  const [root, left, right, join] = ["root", "left", "right", "join"].map((id) => boxOf(layout, identity("sentinel", id)));
  assert.ok(left.x > root.x && right.x > root.x, "branches follow the root");
  assert.equal(left.x, right.x, "sibling branches share one layer");
  assert.notEqual(left.y, right.y, "sibling branches are spread vertically");
  assert.ok(join.x > left.x, "shared join is placed after every incoming branch");
  for (const entry of layout.edges.filter((candidate) => !candidate.back)) {
    assert.ok(layout.byId.get(entry.to)!.box.x > layout.byId.get(entry.from)!.box.x, `forward edge ${entry.edge.id} points right`);
  }
  assert.deepEqual(layout.edges.filter((entry) => entry.back).map((entry) => entry.edge.id).sort(), ["f", "g"], "cycle-closing and self edges are routed as back edges");
});

test("nodes and edge labels never overlap and stay within the reported bounds", () => {
  for (const graph of [scoped(branchedGraph()), scoped(fixtureGraph()), scoped(multiScenarioGraph())]) {
    const layout = layoutAutomationGraph(graph);
    const boxes = [...layout.nodes.map((node) => node.box), ...layout.edges.map((entry) => entry.label)];
    boxes.forEach((box, index) => {
      assert.ok(box.x >= 0 && box.y >= 0 && box.x + box.width <= layout.bounds.width && box.y + box.height <= layout.bounds.height, "box inside bounds");
      for (const other of boxes.slice(index + 1)) assert.ok(!boxesOverlap(box, other), "no node or label overlap");
    });
    assert.equal(layout.edges.length, graph.edges.length, "every scoped edge keeps its own route and label");
  }
});

test("layout is deterministic and a data-only result change keeps every position", () => {
  const graph = scoped(fixtureGraph());
  const first = layoutAutomationGraph(graph);
  const refreshed = layoutAutomationGraph({ ...graph, revision: "revision-b", nodes: graph.nodes.map((node) => ({ ...node, state: "stopped" })), edges: graph.edges.map((edge) => ({ ...edge, result: { status: "success", execution_id: "execution-b", accepted_at: 1 } })) });
  assert.deepEqual(refreshed.nodes.map((node) => [node.id, node.box]), first.nodes.map((node) => [node.id, node.box]));
});

test("a single Sentinel occupies a compact canvas instead of a fixed three-column width", () => {
  const graph = scoped({ ...fixtureGraph(), edges: [] });
  const layout = layoutAutomationGraph(graph);
  assert.equal(layout.nodes.length, 1);
  assert.ok(layout.bounds.width < 2 * layout.nodes[0].box.width, "bounds hug the lone node");
});

let failed = false;
for (const [name, run] of tests) {
  try { run(); console.log(`✓ ${name}`); } catch (error) { failed = true; console.error(`✗ ${name}`); console.error(error); }
}
if (failed) process.exitCode = 1;
else console.log(`${tests.length} automation layout cases passed`);
