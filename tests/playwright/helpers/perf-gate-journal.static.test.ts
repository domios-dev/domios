import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const source = readFileSync(join(process.cwd(), "tests/playwright/suites/00-perf-gate/perf-gate.spec.ts"), "utf8");

test("perf gate emits lifecycle events instead of legacy diagnostic rows", () => {
  assert.doesNotMatch(source, /recordDiagnosticRow\s*\(/);
  assert.doesNotMatch(source, /diagnosticRow\s*\(/);
  assert.doesNotMatch(source, /output-absent/);
});

test("render and capture events retain the R2 observation payloads", () => {
  assert.match(source, /bounce\.record\("render", \{ marker, loadavg: os\.loadavg\(\), focus, viewport \}, tRenderMs\)/);
  assert.match(source, /bounce\.record\("capture", \{ outputPath, .*focus: finalFocus, viewport: finalViewport, status,/);
});

test("perf gate throws the event-state formatter instead of a generic timeout", () => {
  assert.match(source, /throw new Error\(formatPerfFailureState\(bounce, budgetMs\)\)/);
  assert.doesNotMatch(source, /render timeout after/);
});
