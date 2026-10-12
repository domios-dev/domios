import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { actionKind, actionTitle, blueprintActions, blueprintLegend, blueprintNodeKinds, blueprintSources, blueprintSteps, blueprintTriggers, nodeIcon } from "./blueprintKinds";
import { blueprintModel } from "./blueprintModel";
import { typedGraph } from "./blueprintModel.testdata";

const fixture = JSON.parse(readFileSync(new URL("../../../tests/fixtures/blueprint-nodes.golden.json", import.meta.url), "utf8")) as { nodes: { kind: string; type: string }[] };
const catalogTypes = (kind: string) => fixture.nodes.filter((node) => node.kind === kind).map((node) => node.type).sort();
assert.deepEqual(Object.keys(blueprintSteps).sort(), catalogTypes("step"));
assert.equal(blueprintSteps.agent.type, "agent");
assert.deepEqual(Object.keys(blueprintActions).sort(), catalogTypes("handler"));
assert.deepEqual(Object.values(blueprintTriggers).map((kind) => kind.catalogType).sort(), catalogTypes("trigger"));
assert.deepEqual(Object.values(blueprintSources).map((kind) => kind.catalogType).sort(), catalogTypes("source"));
assert.equal(new Set(blueprintLegend).size, Object.keys(blueprintNodeKinds).length);
for (const node of blueprintModel(typedGraph()).nodes) {
  assert.ok(nodeIcon(node));
  assert.ok(blueprintNodeKinds[node.type].width > 0);
  assert.ok(blueprintNodeKinds[node.type].label);
  if (node.action?.handler) assert.ok(actionKind(node.action.handler.type));
}
assert.equal(actionKind("unknown"), undefined);
assert.equal(actionKind("toString"), undefined);
assert.equal(actionTitle("unknown", null, null), "unknown");
console.log("Blueprint kind registry covers the canonical Rust node catalog");
