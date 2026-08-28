import assert from "node:assert/strict";
import test from "node:test";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { gitExecutable, requireBuildIdentity, resolveBuildIdentity } = require("../build-identity.cjs");

test("build_identity_required failed git lookup prevents a frontend identity", () => {
  assert.throws(
    () => resolveBuildIdentity({ env: {}, gitOutput: () => { throw new Error("git unavailable"); } }),
    /git unavailable/,
  );
});

test("build_identity_required Playwright preparation rejects an unknown identity", () => {
  assert.throws(() => requireBuildIdentity("unknown", "frontend build identity"));
  assert.throws(() => requireBuildIdentity("unknown", "backend build identity"));
});

test("build_identity_required normal git output carries its real short hash", () => {
  assert.equal(
    resolveBuildIdentity({ env: {}, gitOutput: () => "a1b2c3d" }),
    "a1b2c3d",
  );
  assert.equal(gitExecutable({}, "linux"), "git");
});

test("build_identity_required explicit frontend hash override remains supported", () => {
  assert.equal(
    resolveBuildIdentity({ env: { CODETERM_FRONTEND_BUILD_HASH: "provided-hash" } }),
    "provided-hash",
  );
});
