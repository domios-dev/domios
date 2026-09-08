import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { prepareLocalInstallCache } from "../local-install-cache.mjs";

function fixture(run) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-local-cache-"));
  const target = path.join(root, "local-install", "a".repeat(40));
  try { run({ root, target }); } finally { fs.rmSync(root, { recursive: true, force: true }); }
}

test("current build-script output retains the warm cache", () => fixture(({ target }) => {
  const owner = path.join(target, "release", "build", "tauri-fixture");
  fs.mkdirSync(owner, { recursive: true });
  fs.writeFileSync(path.join(owner, "root-output"), path.join(owner, "out"));
  assert.equal(prepareLocalInstallCache(target), null);
  assert.ok(fs.existsSync(owner));
}));

test("relocated output is preserved and replaced with an empty owned target", () => fixture(({ root, target }) => {
  const owner = path.join(target, "release", "build", "tauri-fixture");
  fs.mkdirSync(owner, { recursive: true });
  fs.writeFileSync(path.join(owner, "root-output"), path.join(root, "deleted-release", "out"));
  fs.writeFileSync(path.join(target, "owned-artifact"), "opaque-cache-bytes");
  const result = prepareLocalInstallCache(target);
  assert.deepEqual(result.stale, ["tauri-fixture"]);
  assert.equal(path.dirname(result.backup), path.dirname(target));
  assert.equal(fs.readFileSync(path.join(result.backup, "owned-artifact"), "utf8"), "opaque-cache-bytes");
  assert.deepEqual(fs.readdirSync(target), []);
  assert.equal(prepareLocalInstallCache(target), null);
}));

test("a workspace or arbitrary directory is never a cache target", () => fixture(({ root }) => {
  fs.writeFileSync(path.join(root, "source"), "untouched");
  assert.throws(() => prepareLocalInstallCache(root), /canonical local-install/);
  assert.equal(fs.readFileSync(path.join(root, "source"), "utf8"), "untouched");
}));
