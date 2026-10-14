import assert from "node:assert/strict";
import test from "node:test";
import { parseArgs, preferred } from "./driver.mjs";
import { parseRestartArgs } from "./restart.mjs";

test("parseArgs reads driver flags and refuses unknown or missing ones", () => {
  const options = parseArgs(["verify", "--out", "/tmp/x", "--providers", "claude, codex", "--history-kb", "8", "--all-tabs", "--expect-build-change"]);
  assert.equal(options.mode, "verify");
  assert.deepEqual(options.providers, ["claude", "codex"]);
  assert.equal(options.historyBytes, 8192);
  assert.ok(options.allTabs && options.expectBuildChange);
  assert.throws(() => parseArgs(["verify"]), /--out/);
  assert.throws(() => parseArgs(["deploy", "--out", "/tmp/x"]), /mode/);
  assert.throws(() => parseArgs(["setup", "--out", "/tmp/x", "--bogus"]), /flag/);
});

test("preferred picks by capability, ranking the required providers first", () => {
  const run = [{ id: "aider", runtime: false }, { id: "claude", runtime: false }, { id: "codex", runtime: true }];
  assert.equal(preferred(run, false).id, "claude");
  assert.equal(preferred(run, true).id, "codex");
  assert.equal(preferred([{ id: "claude", runtime: false }], true), null);
});

test("parseRestartArgs defaults to a prebuilt restart and accepts the upgrade shape", () => {
  assert.equal(parseRestartArgs(["--out", "/tmp/r"]).start, "dev-prebuilt");
  const upgrade = parseRestartArgs(["--out", "/tmp/r", "--stop-in", "/tmp/old", "--start", "dev", "--label", "upgrade"]);
  assert.equal(upgrade.stopIn, "/tmp/old");
  assert.equal(upgrade.label, "upgrade");
  assert.throws(() => parseRestartArgs(["--out", "/tmp/r", "--start", "build"]), /--start/);
});
