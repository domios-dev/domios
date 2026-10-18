import assert from "node:assert/strict";
import { meshEdgeModel } from "./meshEdgeModel";

const center = { x: 500, y: 500, width: 200, height: 300 };
const peer = { x: 900, y: 700, width: 300, height: 200 };
assert.equal(meshEdgeModel(center, peer, true).path, "M 600 488 C 652.5 488, 697.5 712, 750 712");
assert.equal(meshEdgeModel(center, peer, true).dash, undefined);
assert.equal(meshEdgeModel(center, peer, false).dash, "5 6");
assert.ok(meshEdgeModel(center, peer, false).opacity < meshEdgeModel(center, peer, true).opacity);
assert.equal(meshEdgeModel(center, { ...peer, x: 500, y: 1000 }, true).path, "M 488 650 C 488 737.5, 512 812.5, 512 900");
console.log("5 mesh edge assertions passed");
