/**
 * WHAT: the derived selection is a coverage oracle, not a convenience.
 * WHY: a spec dropped from a scoped file list reads as covered when it never
 * ran, and a zero-match filter exits 0, so the exit code proves nothing.
 */
import assert from "node:assert/strict";
import test from "node:test";
import {
  compareSelection,
  discoveryArgs,
  flattenReport,
  parseJsonReport,
  parseSelectionArgs,
  repoRelativeSpecPaths,
  runArgs,
  selectionFailure,
  type JsonReport,
} from "../../../scripts/run-playwright-selection";

// Separator-neutral so the assertion is about the runner, not the host OS.
const ROOT_DIR = "/repo/tests/playwright";
const REPO_ROOT = "/repo";

const REPORT: JsonReport = {
  config: { rootDir: ROOT_DIR },
  suites: [
    {
      title: "suites/01-smoke/smoke.spec.ts",
      file: "suites/01-smoke/smoke.spec.ts",
      specs: [{ title: "@smoke /api/health is ok", file: "suites/01-smoke/smoke.spec.ts" }],
    },
    {
      title: "suites/12-web/web.spec.ts",
      file: "suites/12-web/web.spec.ts",
      specs: [{ title: "@golden close sync", file: "suites/12-web/web.spec.ts" }],
      suites: [
        {
          title: "peer switching",
          file: "suites/12-web/web.spec.ts",
          specs: [{ title: "@golden routes to the stub", file: "suites/12-web/web.spec.ts" }],
        },
      ],
    },
  ],
};

test("the file-level suite contributes its path, not a title segment", () => {
  assert.deepEqual(
    flattenReport(REPORT).map((entry) => entry.key),
    [
      "suites/01-smoke/smoke.spec.ts :: @smoke /api/health is ok",
      "suites/12-web/web.spec.ts :: @golden close sync",
      "suites/12-web/web.spec.ts :: peer switching > @golden routes to the stub",
    ],
  );
});

test("spec paths are repo-relative and forward-slashed, because the CLI reads them as a regex", () => {
  const files = repoRelativeSpecPaths(REPORT, flattenReport(REPORT), REPO_ROOT);
  assert.deepEqual(files, [
    "tests/playwright/suites/01-smoke/smoke.spec.ts",
    "tests/playwright/suites/12-web/web.spec.ts",
  ]);
  assert.ok(files.every((file) => !file.includes("\\")));
});

test("a spec that was discovered but never ran is named, not silently tolerated", () => {
  const discovered = flattenReport(REPORT);
  const ran = discovered.filter((entry) => !entry.key.includes("close sync"));
  const verdict = compareSelection(discovered, ran);
  assert.equal(verdict.discovered, 3);
  assert.equal(verdict.reported, 2);
  assert.deepEqual(verdict.missing, ["suites/12-web/web.spec.ts :: @golden close sync"]);
  const failure = selectionFailure(verdict);
  assert.ok(failure);
  assert.match(failure.message, /NEVER RAN: suites\/12-web\/web\.spec\.ts :: @golden close sync/);
});

test("a zero-match selection is a failure; an exit code would have called it a pass", () => {
  const verdict = compareSelection([], []);
  assert.equal(verdict.discovered, 0);
  const failure = selectionFailure(verdict);
  assert.ok(failure);
  assert.match(failure.message, /matched no tests/);
});

test("an exact match reports no failure", () => {
  const discovered = flattenReport(REPORT);
  assert.equal(selectionFailure(compareSelection(discovered, [...discovered])), null);
});

test("a test that ran without being discovered is reported too", () => {
  const discovered = flattenReport(REPORT);
  const stray = { file: "suites/99/x.spec.ts", key: "suites/99/x.spec.ts :: @golden stray" };
  const failure = selectionFailure(compareSelection(discovered, [...discovered, stray]));
  assert.ok(failure);
  assert.match(failure.message, /RAN BUT WAS NOT DISCOVERED: suites\/99\/x\.spec\.ts/);
});

test("the JSON report is read from the output file, falling back to stdout", () => {
  const text = JSON.stringify(REPORT);
  assert.equal(parseJsonReport(text, "", "discovery").suites?.length, 2);
  assert.equal(parseJsonReport(null, `Listing tests:\n${text}`, "discovery").suites?.length, 2);
  assert.throws(() => parseJsonReport(null, "", "discovery"), /produced no JSON report/);
});

test("project and grep are required exactly once; nothing else is accepted", () => {
  const parsed = parseSelectionArgs(["--project", "golden", "--grep", "@smoke|close sync"]);
  assert.deepEqual(parsed, { project: "golden", grep: "@smoke|close sync", timeoutMs: undefined });
  assert.deepEqual(parseSelectionArgs(["--", "--project", "full", "--grep", "x"]).project, "full");
  assert.throws(() => parseSelectionArgs(["--project", "golden"]), /--grep must be supplied/);
  assert.throws(() => parseSelectionArgs(["--file", "a", "--project", "g", "--grep", "x"]), /unsupported option --file/);
  assert.throws(() => parseSelectionArgs(["--project", "g", "--grep", "x", "--timeout", "0"]), /positive integer/);
});

test("discovery lists without running, and the run is scoped to the derived files and reuses the instance", () => {
  const input = parseSelectionArgs(["--project", "full", "--grep", "@smoke"]);
  const discovery = discoveryArgs(input);
  assert.ok(discovery.includes("--list"));
  assert.ok(discovery.includes("--reporter=json"));

  const files = ["tests/playwright/suites/01-smoke/smoke.spec.ts"];
  const run = runArgs(input, files);
  assert.ok(!run.includes("--list"));
  assert.ok(run.includes(files[0]));
  assert.ok(run.includes("--workers=1"));
  assert.ok(run.includes("--reporter=line,json"));
  // No --timeout unless the caller asked: the slow web specs set their own in-test.
  assert.ok(!run.some((arg) => arg.startsWith("--timeout=")));
  assert.ok(runArgs(parseSelectionArgs(["--project", "full", "--grep", "x", "--timeout", "45000"]), files).includes("--timeout=45000"));
});
