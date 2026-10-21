import assert from "node:assert/strict";
import { sidebarSectionModel, type SidebarSectionKind } from "./sidebarSectionModel";
for (const kind of ["managers", "workers", "agents", "utilities", "jobs", "services"] satisfies SidebarSectionKind[]) {
  const idle = sidebarSectionModel(kind, 6);
  assert.equal(idle.label, `${kind.toUpperCase()} 6`);
  assert.equal(idle.activity, null);
  assert.equal(idle.accessibleName, idle.label);
  const active = sidebarSectionModel(kind, 6, 4);
  assert.equal(active.activity, "4 working");
  assert.equal(active.accessibleName, `${idle.label}, 4 working`);
}
assert.equal(sidebarSectionModel("workers", 2, 3).working, 2);
assert.equal(sidebarSectionModel("workers", -1, -2).count, 0);
assert.equal(sidebarSectionModel("workers", Number.NaN, Number.NaN).activity, null);
console.log("sidebar section model: 6 group labels, idle/active and invalid counters passed");
