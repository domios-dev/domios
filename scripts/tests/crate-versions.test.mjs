import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

const root = path.resolve(import.meta.dirname, "..", "..");
const tracked = (pattern) => execFileSync("git", ["ls-files", "-z", "--", pattern], { cwd: root, encoding: "utf8" }).split("\0").filter(Boolean);
const appVersion = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8")).version;

// A version moving through an internal crate changes its -C metadata and rebuilds every dependent.
test("only the root binary and the CLI read the compile-time package version", () => {
  const readers = tracked("*.rs").filter((file) => !file.startsWith("src-tauri/src/") && !file.startsWith("cli/src/"))
    .filter((file) => fs.readFileSync(path.join(root, file), "utf8").includes("CARGO_PKG_VERSION"));
  assert.deepEqual(readers, [], "read the app version through codeterm_build_info::version()");
});

test("no internal crate carries the app version", () => {
  const versioned = tracked("src-tauri/crates/*/Cargo.toml").filter((file) =>
    new RegExp(`^version = "${appVersion.replaceAll(".", "\\.")}"$`, "m").test(fs.readFileSync(path.join(root, file), "utf8")));
  assert.deepEqual(versioned, [], "internal crates stay at 0.0.0 or their own version");
});
