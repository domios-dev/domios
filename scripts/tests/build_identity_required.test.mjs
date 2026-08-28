import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { gitExecutable, requireBuildIdentity, resolveBuildIdentity } = require("../build-identity.cjs");
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

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

test("build_identity_required Playwright helper contains no ESM-to-CJS bridge", () => {
  const helper = readFileSync(path.join(root, "tests/playwright/helpers/dev-console.ts"), "utf8");
  assert.doesNotMatch(helper, /import\.meta|createRequire/);
});
