import assert from "node:assert/strict";
import { inventoryGroupVisibility } from "./generalInventoryModel";

for (const expanded of [false, true]) {
  for (const hasShowAll of [false, true]) {
    assert.deepEqual(inventoryGroupVisibility(0, expanded, hasShowAll), { visible: false, open: false, showAll: false });
    assert.deepEqual(inventoryGroupVisibility(2, expanded, hasShowAll), { visible: true, open: expanded, showAll: expanded && hasShowAll });
  }
}
console.log("8 inventory visibility cases passed");
