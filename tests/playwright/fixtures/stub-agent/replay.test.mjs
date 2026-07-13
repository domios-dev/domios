/**
 * WHAT: stub replay consumes provider launch transport and exposes provider readiness.
 * WHY: agent-flow tests must use the same task delivery paths as real provider launches.
 * ORACLE: the subprocess prints the scripted acknowledgement and OpenCode readiness fragments.
 */
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const replay = path.join(here, "replay.mjs");

function run(provider, args, input = "") {
  const result = spawnSync(
    process.execPath,
    [replay, "--stub-provider", provider, "--", ...args],
    {
      encoding: "utf8",
      input,
      env: {
        ...process.env,
        CODETERM_STUB_CAST: path.join(os.tmpdir(), "missing-stub.cast"),
      },
    },
  );
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}

test("Claude replay reads the appended task file before the short launch prompt", () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "ct-replay-task-"));
  const taskFile = path.join(dir, "task.txt");
  writeFileSync(taskFile, "\uFEFFSTUB_ECHO PWWARM-task-file-green");
  try {
    const output = run("claude", [
      "claude",
      "--append-system-prompt-file",
      taskFile,
      "--",
      "Execute the CodeTerm task provided in the appended system prompt.",
    ]);
    assert.match(output, /ack PWWARM-task-file-green/);
    assert.doesNotMatch(output, /received: Execute the CodeTerm task/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("OpenCode replay advertises the provider TUI and accepts its first PTY task", () => {
  const output = run(
    "opencode",
    ["opencode"],
    "STUB_ECHO PWWARM-opencode-green\n",
  );
  assert.match(output, /tab agents · ctrl\+p commands/);
  assert.match(output, /ack PWWARM-opencode-green/);
});
