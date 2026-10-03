import assert from "node:assert/strict";
import { boxesOverlap } from "../Canvas/layout";
import type { Box } from "../Canvas/layout";
import type { ViewportTransform } from "../Canvas/useCanvasViewport";
import { frameAutomation, layoutAutomationGraph, layoutAutomationOrientations, NODE_WIDTH, preferredOrientation, READABLE_SCALE, type AutomationLayout, type CanvasSize } from "./layout";
import { scopedAutomationGraph } from "./model";
import { attachment, chainGraph, decisionGraph, fixtureGraph, fixtureNode, identity, multiScenarioGraph } from "./fixtures.testdata";
import { MAX_ACTIONS_PER_PARTICIPANT, projectAutomationGraph } from "./projection";
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

test("vertical flow runs top to bottom with the same branch, join and cycle semantics", () => {
  const horizontal = layoutAutomationGraph(scoped(branchedGraph()));
  const layout = layoutAutomationGraph(scoped(branchedGraph()), "vertical");
  const [root, left, right, join] = ["root", "left", "right", "join"].map((id) => boxOf(layout, identity("sentinel", id)));
  assert.ok(left.y > root.y + root.height && right.y > root.y + root.height, "branches sit below the root");
  assert.equal(left.y, right.y, "sibling branches share one layer");
  assert.ok(left.x + left.width <= right.x || right.x + right.width <= left.x, "sibling branches are spread side by side");
  assert.ok(join.y > left.y + left.height, "shared join is placed after every incoming branch");
  for (const entry of layout.edges.filter((candidate) => !candidate.back)) {
    const from = layout.byId.get(entry.from)!.box;
    assert.ok(layout.byId.get(entry.to)!.box.y >= from.y + from.height, `forward edge ${entry.edge.id} points down`);
  }
  assert.deepEqual(layout.edges.map((entry) => [entry.edge.id, entry.back]), horizontal.edges.map((entry) => [entry.edge.id, entry.back]), "back-edge classification is orientation independent");
  assert.deepEqual([...layout.byId.keys()], [...horizontal.byId.keys()], "exact identities are preserved");
});

function pathPoints(path: string) {
  const numbers = (path.match(/-?\d+(?:\.\d+)?(?:e-?\d+)?/g) ?? []).map(Number);
  assert.ok(numbers.length >= 8 && numbers.length % 2 === 0, "path is a finite cubic");
  return Array.from({ length: numbers.length / 2 }, (_, index) => ({ x: numbers[index * 2], y: numbers[index * 2 + 1] }));
}

test("nodes, edge curves and labels never overlap and stay within the reported bounds in both orientations", () => {
  for (const graph of [scoped(branchedGraph()), scoped(fixtureGraph()), scoped(multiScenarioGraph()), scoped(chainGraph()), scoped(decisionGraph())]) {
    for (const layout of Object.values(layoutAutomationOrientations(graph))) {
      const inside = (x: number, y: number) => Number.isFinite(x) && Number.isFinite(y) && x >= 0 && y >= 0 && x <= layout.bounds.width && y <= layout.bounds.height;
      const boxes = [...layout.nodes.map((node) => node.box), ...layout.edges.map((entry) => entry.label)];
      boxes.forEach((box, index) => {
        assert.ok(inside(box.x, box.y) && inside(box.x + box.width, box.y + box.height), `${layout.orientation} box inside bounds`);
        for (const other of boxes.slice(index + 1)) assert.ok(!boxesOverlap(box, other), `${layout.orientation} no node or label overlap`);
      });
      for (const entry of layout.edges) for (const point of pathPoints(entry.path)) assert.ok(inside(point.x, point.y), `${layout.orientation} ${entry.edge.id} curve hull inside bounds`);
      assert.deepEqual(layout.edges.flatMap((entry) => entry.edge.source ? [entry.edge.source] : []), [...graph.edges], "every scoped edge keeps its own route and label");
    }
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

function project(box: Box, camera: ViewportTransform): Box {
  return { x: box.x * camera.scale + camera.x, y: box.y * camera.scale + camera.y, width: box.width * camera.scale, height: box.height * camera.scale };
}

function within(box: Box, size: CanvasSize) {
  return box.x >= 0 && box.y >= 0 && box.x + box.width <= size.width && box.y + box.height <= size.height;
}

function opened(graph: ReturnType<typeof scoped>, size: CanvasSize, whole = false): { layout: AutomationLayout; camera: ViewportTransform } {
  const layouts = layoutAutomationOrientations(graph);
  const layout = layouts[preferredOrientation(layouts, size)];
  const camera = frameAutomation(layout, size, whole);
  assert.ok(camera, "a measured canvas yields a camera");
  return { layout, camera };
}

function chainBoxes(layout: AutomationLayout) {
  return ["chain-root", "chain-stage", "chain-worker"].map((id, index) => boxOf(layout, identity(index === 2 ? "worker" : "sentinel", id)));
}

test("a 500px measured canvas opens the three-node chain top to bottom with readable cards and full labels", () => {
  const graph = scoped(chainGraph());
  for (const size of [{ width: 500, height: 480 }, { width: 500, height: 576 }]) {
    const { layout, camera } = opened(graph, size);
    assert.ok(camera.scale >= READABLE_SCALE, `scale ${camera.scale} is readable at ${size.height}px`);
    const cards = chainBoxes(layout).map((box) => project(box, camera));
    for (const card of cards) {
      assert.ok(within(card, size), "every card is fully visible on first open");
      assert.ok(card.width >= NODE_WIDTH * READABLE_SCALE, `card width ${card.width} meets the readable threshold`);
    }
    cards.slice(1).forEach((card, index) => assert.ok(card.y >= cards[index].y + cards[index].height, "chain flows downward"));
    for (const entry of layout.edges) {
      assert.ok(within(project(entry.label, camera), size), "edge label is visible");
      assert.deepEqual(entry.label.lines, [entry.edge.bucket, `${entry.edge.handler_type} · ${entry.edge.status}`], "label carries the supplied branch and outcome unclipped");
    }
  }
});

test("desktop canvases keep the chain left to right at a readable scale", () => {
  const graph = scoped(chainGraph());
  for (const size of [{ width: 980, height: 512 }, { width: 1400, height: 576 }]) {
    const { layout, camera } = opened(graph, size);
    assert.ok(camera.scale >= READABLE_SCALE);
    const cards = chainBoxes(layout).map((box) => project(box, camera));
    for (const card of cards) assert.ok(within(card, size));
    cards.slice(1).forEach((card, index) => assert.ok(card.x >= cards[index].x + cards[index].width, "chain flows rightward"));
  }
});

test("a long chain opens on a readable root and the explicit overview still fits everything", () => {
  const ids = Array.from({ length: 8 }, (_, index) => identity("sentinel", `long-${index}`));
  const graph = scoped({
    ...fixtureGraph(),
    scenarios: [{ id: "scenario-long", name: "scenario-long", root: ids[0] }],
    nodes: ids.map((target) => ({ ...fixtureNode(target, `name-${target.id}`), sentinel_definition: definition(target) })),
    edges: ids.slice(1).map((target, index) => route(`long-${index}`, ids[index], target)),
  });
  const size = { width: 500, height: 480 };
  const framed = opened(graph, size);
  const whole = opened(graph, size, true);
  assert.ok(whole.camera.scale < READABLE_SCALE, "the overview alone would be too small to read");
  assert.equal(framed.camera.scale, READABLE_SCALE);
  const root = project(boxOf(framed.layout, ids[0]), framed.camera);
  assert.ok(within(root, size) && root.width >= NODE_WIDTH * READABLE_SCALE, "root is framed fully at a readable size");
  assert.ok(within(project(whole.layout.bounds, whole.camera), size), "explicit overview contains every node, edge and label");
});

test("every declared outcome handler is drawn once, target-less actions become keyed leaves and no identity is invented", () => {
  const graph = scoped(decisionGraph());
  const view = projectAutomationGraph(graph);
  const participants = view.nodes.flatMap((node) => node.participant ? [node.key] : []);
  assert.deepEqual(participants, graph.nodes.map((node) => automationKey(node.identity)), "participants are exactly the authoritative nodes");
  const keys = view.nodes.map((node) => node.key);
  assert.equal(new Set(keys).size, keys.length, "presentation keys are unique");
  for (const node of graph.nodes) {
    const source = automationKey(node.identity);
    for (const outcome of node.outcomes) {
      const declared = outcome.handlers.length ? outcome.handlers.map((handler) => handler.id) : [null];
      for (const handlerId of declared) {
        const real = graph.edges.filter((edge) => automationKey(edge.source) === source && edge.bucket === outcome.bucket && edge.handler_id === handlerId);
        const drawn = view.edges.filter((edge) => edge.from === source && edge.bucket === outcome.bucket && edge.handler_id === handlerId);
        assert.equal(drawn.length, 1, `${outcome.bucket}/${handlerId} drawn exactly once`);
        assert.equal(drawn[0].source, real[0] ?? null, "an exact graph edge is reused instead of duplicated");
        const handler = outcome.handlers.find((entry) => entry.id === handlerId);
        const leaf = view.nodes.find((entry) => entry.key === drawn[0].to)?.action;
        if (handler?.target) assert.equal(drawn[0].to, automationKey(handler.target), "a declared participant target stays the one shared identity");
        else {
          assert.ok(leaf, "target-less handler or empty outcome becomes an action leaf");
          assert.equal(leaf.sourceKey, source);
          assert.equal(leaf.bucket, outcome.bucket);
          assert.equal(leaf.handler, handler ?? null, "leaf maps back to its exact source handler");
        }
      }
    }
  }
  const leaf = (handlerId: string) => view.nodes.find((entry) => entry.action?.handler?.id === handlerId)!.action!;
  const root = graph.nodes[0];
  assert.equal(leaf("marker-d").taken, true, "handler in the last accepted result is the taken path");
  assert.equal(leaf("marker-d").result, root.last_result!.handlers[0], "leaf carries the exact accepted execution result");
  for (const id of ["notify-d", "stop-d", "cleanup-e"]) {
    assert.equal(leaf(id).taken, false, `${id} is an untaken or unrun branch`);
    assert.equal(leaf(id).result, null, `${id} has no invented result`);
  }
  assert.equal(view.edges.filter((edge) => edge.evidence !== "declared").length, graph.edges.length, "real configured and observed evidence is unchanged");
});

test("a declared handler whose target is an existing participant routes to it without an exact graph edge", () => {
  const graph = scoped(fixtureGraph());
  const view = projectAutomationGraph(graph);
  const root = graph.nodes[0];
  const handler = root.outcomes[0].handlers[1];
  const routed = view.edges.filter((edge) => edge.handler_id === handler.id && edge.bucket === root.outcomes[0].bucket);
  assert.equal(routed.length, 1);
  assert.equal(routed[0].to, automationKey(handler.target!));
  assert.equal(view.nodes.filter((node) => node.action).length, 0, "no action leaf stands in for a real participant");
});

test("fanout beyond the bound folds into one counted overflow leaf and stays finite", () => {
  const graph = scoped(fixtureGraph());
  const count = MAX_ACTIONS_PER_PARTICIPANT + 5;
  const handlers = Array.from({ length: count }, (_, index) => ({ id: `bulk-${index}`, type: "notify", target: null, availability: "available" as const, last_result: null }));
  const wide = { ...graph, nodes: graph.nodes.map((node, index) => index === 0 ? { ...node, outcomes: [{ bucket: "bulk", handlers }] } : node) };
  const view = projectAutomationGraph(wide);
  const actions = view.nodes.flatMap((node) => node.action ? [node.action] : []);
  assert.equal(actions.filter((action) => action.handler).length, MAX_ACTIONS_PER_PARTICIPANT);
  assert.deepEqual(actions.filter((action) => action.overflow).map((action) => action.overflow), [5], "the hidden remainder is counted, not dropped");
  for (const layout of Object.values(layoutAutomationOrientations(wide))) {
    assert.ok(Number.isFinite(layout.bounds.width) && Number.isFinite(layout.bounds.height));
    assert.equal(layout.nodes.length, view.nodes.length);
  }
});

test("action leaves sit after their source in both orientations", () => {
  for (const layout of Object.values(layoutAutomationOrientations(scoped(decisionGraph())))) {
    layout.items.forEach((item, index) => {
      if (!item.action) return;
      const source = layout.byId.get(item.action.sourceKey)!.box;
      const box = layout.nodes[index].box;
      assert.ok(layout.orientation === "horizontal" ? box.x >= source.x + source.width : box.y >= source.y + source.height, `${layout.orientation} leaf follows its source`);
    });
  }
});

let failed = false;
for (const [name, run] of tests) {
  try { run(); console.log(`✓ ${name}`); } catch (error) { failed = true; console.error(`✗ ${name}`); console.error(error); }
}
if (failed) process.exitCode = 1;
else console.log(`${tests.length} automation layout cases passed`);
