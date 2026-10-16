import assert from "node:assert/strict";
import { frontendPrototypeRoute } from "./frontendPrototypeRoute";

for (const route of ["mission-control", "gallery", "snake"] as const) {
  assert.equal(frontendPrototypeRoute(`?prototype=${route}`), route);
}
assert.equal(frontendPrototypeRoute(""), null);
assert.equal(frontendPrototypeRoute("?prototype=unknown"), null);
assert.equal(frontendPrototypeRoute("?prototype=snake&prototype=gallery"), "snake");
