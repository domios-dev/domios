import assert from "node:assert/strict";
import { readFileSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
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
    resolveBuildIdentity({ env: { CODETERM_FRONTEND_BUILD_HASH: "provided-hash" },
      gitOutput: () => { throw new Error("override must not read HEAD"); } }),
    "provided-hash",
  );
});

test("build_identity_required invalid override refuses without falling back to HEAD", () => {
  for (const hash of ["", "unknown", " "]) {
    assert.throws(() => resolveBuildIdentity({ env: { CODETERM_FRONTEND_BUILD_HASH: hash },
      gitOutput: () => { throw new Error("must not read HEAD"); } }), /non-unknown build identity/);
  }
});

test("build_identity_required Rust launch override precedes unavailable git", () => {
  const directory = mkdtempSync(path.join(tmpdir(), "build-identity-rust-"));
  try {
    const source = readFileSync(path.join(root, "src-tauri/build.rs"), "utf8");
    const embed = source.match(/^fn embed_build_commit\(\) \{[\s\S]*?^\}/m)[0];
    const file = path.join(directory, "identity.rs");
    const binary = path.join(directory, process.platform === "win32" ? "identity.exe" : "identity");
    writeFileSync(file, `${embed}\nfn main() { embed_build_commit(); }\n`);
    const compiled = spawnSync("rustc", ["--edition=2021", file, "-o", binary], { encoding: "utf8" });
    assert.equal(compiled.status, 0, compiled.stderr);
    const sha = "a".repeat(40);
    const env = { ...process.env, PATH: "", CODETERM_BUILD_SHA: sha, CODETERM_BUILD_HASH: sha.slice(0, 9) };
    const overridden = spawnSync(binary, [], { env, encoding: "utf8" });
    assert.equal(overridden.status, 0, overridden.stderr);
    assert.ok(overridden.stdout.includes(`cargo:rustc-env=BUILD_SHA=${sha}\n`));
    assert.ok(overridden.stdout.includes(`cargo:rustc-env=BUILD_HASH=${sha.slice(0, 9)}\n`));
    delete env.CODETERM_BUILD_SHA;
    delete env.CODETERM_BUILD_HASH;
    const fallback = spawnSync(binary, [], { env, encoding: "utf8" });
    assert.equal(fallback.status, 0, fallback.stderr);
    assert.match(fallback.stdout, /cargo:rustc-env=BUILD_HASH=unknown/);
    const invalid = spawnSync(binary, [], { env: { ...env, CODETERM_BUILD_SHA: sha }, encoding: "utf8" });
    assert.notEqual(invalid.status, 0);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("build_identity_required Playwright helper contains no ESM-to-CJS bridge", () => {
  const helper = readFileSync(path.join(root, "tests/playwright/helpers/dev-console.ts"), "utf8");
  assert.doesNotMatch(helper, /import\.meta|createRequire/);
});
