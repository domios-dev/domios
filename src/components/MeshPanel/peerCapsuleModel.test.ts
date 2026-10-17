import assert from "node:assert/strict";
import { buildInventoryRows } from "./meshInventoryModel";
import { peerCapsuleModel } from "./peerCapsuleModel";
import { daemon, schedule } from "../General/generalOverview.testdata";

const rows = buildInventoryRows(Array.from({ length: 20 }, (_, id) => ({ ...daemon, id: String(id), name: `Service with long name ${id}`, provenance: { freshness: "fresh", offline: false } })));
const model = peerCapsuleModel(rows, "ready");
assert.equal(model.lines.length, 2);
assert.equal(model.more, 17);
assert.equal(model.lines.flat().length + model.more, rows.length);
for (const [index, line] of model.lines.entries()) {
  const widths = line.map((capsule) => capsule.width);
  if (index === model.lines.length - 1 && model.more) widths.push(model.moreWidth);
  assert.ok(widths.reduce((sum, width) => sum + width, 0) + Math.max(0, widths.length - 1) * 4 <= 330);
}
assert.equal(peerCapsuleModel([], "unavailable").more, 0);
assert.equal(peerCapsuleModel(rows.slice(0, 1), "unavailable").lines[0][0].muted, true);
assert.equal(peerCapsuleModel(rows.slice(0, 1), "ready").lines[0][0].muted, false);
const states = peerCapsuleModel(buildInventoryRows([{ ...daemon, health: { status: "down" } }, { ...schedule, enabled: false }]), "ready").lines.flat();
assert.equal(states[0].color, "var(--ct-red)");
assert.equal(states[1].color, "var(--ct-muted)");
assert.equal(rows.length, 20);
console.log("Capsule packing: two bounded rows, overflow accounting, stale state and service/cron dots passed");
