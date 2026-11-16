import assert from "node:assert/strict";
import Graph from "graphology";
import { edgeKey, longEdgeFactor, mutualTopEdges, restEdgeStrength, SettledEdgeUpdates } from "./memoryEdgePolicy";
import { applyEgoEdges, applyRealEdges, settleMemoryEdges, synchronizeMemoryEgo } from "./memoryGraphEdges";
import { deriveMemoryGraphPalette } from "./memoryGraphTheme";
import type { SnapshotEdge } from "./snapshotTypes";

for (const [ratio, strength] of [[1, null], [0.701, null], [0.7, 0.1], [0.4, 0.1], [0.399, 0.14], [0.15, 0.14], [0.149, 0.18], [0.05, 0.18]]) {
  assert.equal(restEdgeStrength(ratio!), strength);
}
assert.equal(longEdgeFactor(350, 800, 600), 1);
assert.equal(longEdgeFactor(351, 800, 600), 0.5);

const edge = (from: string, to: string, confidence: number, rel = "related"): SnapshotEdge => ({ from, to, confidence, rel });
const fan = [edge("hub", "a", 0.9), edge("hub", "b", 0.8), edge("hub", "c", 0.7), edge("hub", "d", 0.1)];
const peers = [edge("a", "b", 1), edge("a", "c", 1), edge("a", "d", 1), edge("b", "c", 1), edge("b", "d", 1), edge("c", "d", 1)];
const input = [...fan, ...peers];
const retained = mutualTopEdges(input);
assert.equal(retained.length, 7);
assert.deepEqual(retained.filter(row => row.from === "hub"), [fan[0]]);
assert.deepEqual(mutualTopEdges([...input].reverse()), retained);
assert.deepEqual(mutualTopEdges([...input, ...input]), retained);
assert.deepEqual(mutualTopEdges(fan), fan);
const tied = ["z", "m", "a", "q"].map(rel => edge("a", "b", 1, rel));
assert.deepEqual(mutualTopEdges(tied).map(row => row.rel), ["a", "m", "q"]);
assert.notEqual(edgeKey(edge("a→b", "c", 1)), edgeKey(edge("a", "b→c", 1)));

const graph = new Graph({ multi: true });
for (const [index, id] of ["hub", "a", "b", "c", "d"].entries()) graph.addNode(id, { x: index * 0.2, y: 0 });
const store = new Map<string, SnapshotEdge[]>();
for (const row of input) {
  for (const id of [row.from, row.to]) store.set(id, [...store.get(id) ?? [], row]);
}
const palette = deriveMemoryGraphPalette("#000000", "#ffffff", "#646464");
let ratio = 0.2;
const render = () => {
  settleMemoryEdges(graph, store, palette, { ratio, width: 800, height: 600, project: point => ({ x: point.x * 1000, y: point.y }) }, false);
  synchronizeMemoryEgo(graph, store, palette, null, null);
};
render();
const resting = new Map(graph.edges().map(key => [key, { ...graph.getEdgeAttributes(key) }]));
assert.equal(graph.size, 7);
for (const attrs of resting.values()) {
  assert.equal(attrs.size, 0.6);
  assert.equal(attrs.curvature, 0.15);
  assert.ok(["rgb(14,14,14)", "rgb(7,7,7)"].includes(attrs.color));
}
synchronizeMemoryEgo(graph, store, palette, null, "hub");
assert.equal(graph.edges("hub").length, 4);
for (const row of peers) assert.deepEqual(graph.getEdgeAttributes(edgeKey(row)), resting.get(edgeKey(row)));
for (const row of fan) {
  assert.equal(graph.getEdgeAttribute(edgeKey(row), "color"), palette.hoverEdgeColor);
  assert.equal(graph.getEdgeAttribute(edgeKey(row), "size"), 1);
}
synchronizeMemoryEgo(graph, store, palette, "hub", "a");
for (const row of peers) assert.equal(graph.getEdgeAttribute(edgeKey(row), "visibility"), "hidden");
for (const row of fan) {
  assert.equal(graph.getEdgeAttribute(edgeKey(row), "color"), palette.selectedEdgeColor);
  assert.equal(graph.getEdgeAttribute(edgeKey(row), "size"), 1.25);
  assert.equal(graph.getEdgeAttribute(edgeKey(row), "zIndex"), 1);
}
synchronizeMemoryEgo(graph, store, palette, null, null);
assert.deepEqual(new Map(graph.edges().map(key => [key, graph.getEdgeAttributes(key)])), resting);

const updates = new SettledEdgeUpdates();
updates.beginGesture();
ratio = 0.1;
assert.equal(updates.update(render), false);
assert.equal(updates.update(() => synchronizeMemoryEgo(graph, store, palette, null, "hub")), false);
assert.deepEqual(new Map(graph.edges().map(key => [key, graph.getEdgeAttributes(key)])), resting);
updates.settle(render);
assert.equal(graph.getEdgeAttribute(edgeKey(peers[0]), "color"), "rgb(18,18,18)");
for (const key of graph.edges()) graph.dropEdge(key);
applyRealEdges(graph, store, palette);
applyEgoEdges(graph, null, store, palette);
assert.equal(graph.size, 7);

ratio = 0.8;
render();
assert.equal(graph.size, 0);
synchronizeMemoryEgo(graph, store, palette, null, "hub");
assert.equal(graph.size, 4);
synchronizeMemoryEgo(graph, store, palette, null, null);
assert.equal(graph.size, 0);
graph.addEdgeWithKey("c0-1", "a", "b");
settleMemoryEdges(graph, store, palette, { ratio: 2, width: 800, height: 600, project: point => point }, true);
synchronizeMemoryEgo(graph, store, palette, null, null);
assert.equal(graph.size, 1);
assert.equal(graph.getEdgeAttribute("c0-1", "color"), "rgb(8,8,8)");
assert.equal(graph.getEdgeAttribute("c0-1", "size"), 0.6);
console.log("Zoom bands, mutual top-3/fallback, long edges, full ego emphasis, cluster edges and frozen gesture appearance passed");
