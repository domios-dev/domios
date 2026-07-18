import assert from "node:assert/strict";
import { developmentBuildLabel, isDevelopmentFrontendBuild } from "./buildIdentity";

assert.equal(developmentBuildLabel(true, "1.7.1"), "DEV · v1.7.1");
assert.equal(developmentBuildLabel(false, "1.7.1"), null);
assert.equal(isDevelopmentFrontendBuild("serve", {}), true);
assert.equal(isDevelopmentFrontendBuild("build", { CODETERM_FRONTEND_DEV: "1" }), true);
assert.equal(isDevelopmentFrontendBuild("build", { CODETERM_DEV: "1" }), false);

console.log("titlebar build identity tests: passed");
