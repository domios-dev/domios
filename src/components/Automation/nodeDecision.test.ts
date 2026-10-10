import assert from "node:assert/strict";
import { fixtureGraph } from "./fixtures.testdata";
import { nodeKind, bucketDefinitions } from "./nodeDecision";
import { graphRuns } from "./blueprintRunModel";

const graph = fixtureGraph();
const classifier = { adapter_id: "typesafe-jev", model: "jev-latest" };
const node = { ...graph.nodes[0], detail: { definition: {
  probe: "build-command", question: "Choose clean: build finished with no problems; warnings: build finished with warnings; broken: build failed.",
  buckets: [{ name: "clean", pattern: null }, { name: "warnings", pattern: null }, { name: "broken", pattern: null }],
} } };
assert.deepEqual(bucketDefinitions(node), [
  { name: "clean", description: "" },
  { name: "warnings", description: "" },
  { name: "broken", description: "" },
]);
assert.deepEqual(nodeKind(node, classifier), { ai: true, label: "Jev · jev-latest", detail: "typesafe-jev" });
const pattern = { ...node, detail: { definition: { ...node.detail.definition, buckets: [{ name: "clean", pattern: "^ok$" }] } } };
assert.equal(bucketDefinitions(pattern)[0].description, "Matches pattern: ^ok$");
assert.equal(bucketDefinitions({ ...node, detail: { definition: { question: "Unstructured contract", buckets: [{ name: "clean", pattern: null }] } } })[0].description, "");
assert.equal(nodeKind(graph.nodes[2]).label, "Provider-fixture-a · model-fixture-a");
graph.nodes = [{ ...node, history: [
  { id: "picked", bucket: "warnings", accepted_at: 1, decided_by: "adapter", adapter_id: classifier.adapter_id, model: "recorded-model", confidence: 0.87, handlers: [] },
  { id: "pattern", bucket: "clean", accepted_at: 2, decided_by: "pattern", confidence: 1, handlers: [] },
] }];
graph.configured_fallback = classifier;
const runs = graphRuns(graph);
assert.equal(runs[0].events[0].classifier?.label, "Pattern rules");
assert.equal(runs[1].events[0].classifier?.label, "Jev · recorded-model");
assert.equal(runs[1].events[0].outcome, "warnings");
assert.equal(runs[1].events[0].confidence, 0.87);
console.log("Stored route contracts, configured node kinds and actual historical classifier provenance passed");

assert.equal(nodeKind(pattern, classifier).ai, false);
assert.equal(nodeKind({ ...node, detail: { definition: { unavailable: "pipeline" } } }, classifier).ai, false);

assert.equal(nodeKind(node).ai, true);
assert.equal(nodeKind({ ...node, detail: { definition: { decision_kind: "pattern", question: "opaque", buckets: [{ name: "ok", pattern: null }] } } }, classifier).ai, false);
