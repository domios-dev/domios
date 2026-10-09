import assert from "node:assert/strict";
import fixture from "./blueprintKinds.rust.testdata.json";
import { actionKind, actionTitle, blueprintActions, blueprintLegend, blueprintNodeKinds, blueprintSteps, blueprintTriggers, nodeIcon } from "./blueprintKinds";
import { blueprintModel } from "./blueprintModel";
import { typedGraph } from "./blueprintModel.testdata";

assert.deepEqual(Object.keys(blueprintSteps).sort(), [...fixture.steps].sort());
assert.deepEqual(Object.keys(blueprintActions).sort(), fixture.routes.map((kind) => kind === "sentinel" ? "blueprint" : kind).sort());
for (const kind of fixture.triggers) assert.ok(kind === "input" ? blueprintTriggers.manual : blueprintTriggers[kind as keyof typeof blueprintTriggers]);
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
console.log("Blueprint kind registry covers the Rust StepKind, Route and Trigger fixture");
