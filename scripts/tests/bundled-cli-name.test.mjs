// The staged CLI name is the contract between two producers — a local
// `install-local` and the cross-building release workflow — and the consumer,
// the app's startup lookup. When they disagreed, an app installed from CI kept a
// stale triple-named resource that the lookup prefers, so a freshly bundled CLI
// was ignored and every `codeterm` command kept talking an old protocol.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { bundledCliName, clearStaleBundledCli, hostWindowsTarget } from "../bundle-cli.js";

test("windows stages one name whether or not a cross target is set", () => {
  const native = bundledCliName({}, "win32", "x64");
  const cross = bundledCliName({ CARGO_BUILD_TARGET: "x86_64-pc-windows-msvc" }, "win32", "x64");
  assert.equal(native, cross, "a local build and a cross build must agree on the name");
  assert.match(native, /^codeterm-.+-pc-windows-msvc\.exe$/);
});

test("an explicit cross target wins over the host arch", () => {
  assert.equal(
    bundledCliName({ CARGO_BUILD_TARGET: "aarch64-pc-windows-msvc" }, "win32", "x64"),
    "codeterm-aarch64-pc-windows-msvc.exe",
  );
});

test("each windows arch maps to its own triple", () => {
  assert.equal(hostWindowsTarget("x64"), "x86_64-pc-windows-msvc");
  assert.equal(hostWindowsTarget("arm64"), "aarch64-pc-windows-msvc");
  assert.equal(hostWindowsTarget("ia32"), "i686-pc-windows-msvc");
});

test("non-windows keeps the plain name", () => {
  assert.equal(bundledCliName({}, "linux", "x64"), "codeterm");
  assert.equal(bundledCliName({ CARGO_BUILD_TARGET: "aarch64-apple-darwin" }, "darwin", "arm64"), "codeterm");
});

test("bundle regeneration clears stale CLI names before Tauri packages resources", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-cli-stage-"));
  try {
    fs.writeFileSync(path.join(dir, "codeterm-09-18.exe"), "stale");
    fs.writeFileSync(path.join(dir, "codeterm.exe"), "old");
    fs.writeFileSync(path.join(dir, "keep.txt"), "keep");

    clearStaleBundledCli(dir);

    assert.deepEqual(fs.readdirSync(dir), ["keep.txt"]);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
