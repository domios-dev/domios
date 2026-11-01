import assert from "node:assert/strict";
import { developmentBuildLabel } from "./buildIdentity";

assert.equal(developmentBuildLabel(true, "1.7.1"), "DEV · v1.7.1");
assert.equal(developmentBuildLabel(false, "1.7.1"), null);

console.log("titlebar build identity tests: passed");
