import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { parse } from "yaml";

const root = resolve(import.meta.dirname, "../..");
const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";
function scan(args, failure = "0") {
  const directory = mkdtempSync(join(tmpdir(), "secret-scan-"));
  const calls = join(directory, "calls");
  try {
    const result = spawnSync(bash, ["-c", `
gitleaks() { printf '%s\\n' "$*" >> "$SCAN_CALLS"; return "$SCAN_FAILURE"; }
export -f gitleaks
bash scripts/secret-scan.sh "$@"
`, "--", ...args], {
      cwd: root, encoding: "utf8",
      env: { ...process.env, SCAN_CALLS: calls.replaceAll("\\", "/"), SCAN_FAILURE: failure },
    });
    let invoked = [];
    try { invoked = readFileSync(calls, "utf8").trim().split(/\r?\n/); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
    return { ...result, invoked };
  } finally { rmSync(directory, { recursive: true, force: true }); }
}

test("missing, empty and zero bases fail before invoking or installing the scanner", () => {
  for (const args of [[], [""], ["0".repeat(40)]]) {
    const result = scan(args);
    assert.equal(result.status, 2, result.stderr);
    assert.match(result.stderr, /nonzero base commit is required/);
    assert.deepEqual(result.invoked, []);
  }
});

test("an unavailable base fails closed", () => {
  const result = scan(["missing-secret-scan-base"]);
  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stderr, /not fetched/);
  assert.deepEqual(result.invoked, []);
});

test("a valid base scans only its commit range and the working tree with redaction", () => {
  const result = scan(["HEAD"]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.invoked.length, 2);
  assert.match(result.invoked[0], /^git .*--redact .*--log-opts=HEAD\.\.HEAD \.$/);
  assert.match(result.invoked[1], /^dir .*--redact .*\.$/);
  assert.ok(result.invoked.every(call => !call.includes("--all")));
});

test("a failed commit scan preserves failure and stops before the working tree scan", () => {
  const result = scan(["HEAD"], "9");
  assert.equal(result.status, 9, result.stderr);
  assert.equal(result.invoked.length, 1);
});

test("repository guards stop on preflight failure before selecting or scanning a base", () => {
  const workflow = parse(readFileSync(join(root, ".github/workflows/tests.yml"), "utf8"));
  const steps = workflow.jobs.policy.steps;
  const preflight = steps.findIndex(step => step.name === "Embedded docs and generated contract preflight");
  const selection = steps.findIndex(step => step.id === "change");
  const scanner = steps.findIndex(step => step.run?.startsWith("bash scripts/secret-scan.sh"));
  assert.ok(preflight >= 0 && selection > preflight && scanner > selection);
  for (const step of steps.slice(preflight, scanner + 1)) {
    assert.ok(step.if === undefined || step.if === "${{ success() }}", step.name);
  }
  assert.equal(steps[scanner].env.SCAN_BASE, "${{ steps.change.outputs.base }}");
});
