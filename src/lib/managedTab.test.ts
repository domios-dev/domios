import assert from "node:assert/strict";
import { managedTabPresentation } from "./managedTab";

assert.deepEqual(managedTabPresentation(null), { locked: false, label: null });
const managed = { manager_tab_id: "owner", manager_name: "Scout", manager_closed: false };
assert.equal(managedTabPresentation(managed).locked, true);
assert.equal(managedTabPresentation(managed).label, "Managed by Scout");
assert.equal(managedTabPresentation({ ...managed, manager_closed: true }).locked, false);
assert.match(managedTabPresentation({ ...managed, manager_closed: true }).label ?? "", /Scout/);
console.log("managedTab: 4 cases passed");
