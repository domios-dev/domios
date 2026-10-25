import assert from "node:assert/strict";
import { blueprintNavigation, restoreBlueprintNavigation } from "./blueprintNavigation";
import { viewerPipeline } from "../components/Automation/blueprintViewer.testdata";

const pipeline = viewerPipeline();
const route = { runId: "past", trail: ["invoke"], selected: '["invoke","return-count"]' };
assert.deepEqual(blueprintNavigation(JSON.parse(JSON.stringify(route))), route);
assert.equal(blueprintNavigation({ runId: null, trail: Array(17).fill("call") }), undefined);
assert.equal(blueprintNavigation({ runId: {}, trail: [] }), undefined);
assert.equal(blueprintNavigation({ runId: null, trail: [3] }), undefined);
assert.deepEqual(restoreBlueprintNavigation(route, pipeline), route);
assert.deepEqual(restoreBlueprintNavigation({ ...route, runId: "removed" }, pipeline), { runId: null, trail: [], selected: null });
assert.deepEqual(restoreBlueprintNavigation({ runId: null, trail: ["invoke", "removed"], selected: null }, pipeline), { runId: null, trail: ["invoke"], selected: null });
assert.deepEqual(restoreBlueprintNavigation(route, { ...pipeline, expanded_calls: [], runs: [] }), { runId: null, trail: [], selected: null });
assert.equal(restoreBlueprintNavigation({ ...route, selected: '["invoke","removed"]' }, pipeline).selected, null);
console.log("blueprintNavigation: 9 assertions passed");
