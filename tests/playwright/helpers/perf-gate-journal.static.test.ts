import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const source = readFileSync(join(process.cwd(), "tests/playwright/suites/00-perf-gate/perf-gate.spec.ts"), "utf8");

function renderMetricUsesTransportCleanSend(source: string): boolean {
  return /await api\.post\(\s*`\/panes\/\$\{paneId\}\/send`,\s*\{\s*text: `STUB_ECHO \$\{marker\}`,\s*enter: true,\s*raw: true\s*\}\s*\)/s.test(source);
}

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

test("render metric send is transport-clean rather than a default user turn", () => {
  assert.ok(renderMetricUsesTransportCleanSend(source), "render metric send must set raw: true");
  const defaultUserTurn = source.replace(
    "{ text: `STUB_ECHO ${marker}`, enter: true, raw: true }",
    "{ text: `STUB_ECHO ${marker}`, enter: true }",
  );
  assert.notEqual(defaultUserTurn, source, "mutation must remove raw: true");
  assert.equal(renderMetricUsesTransportCleanSend(defaultUserTurn), false, "default user-turn body must fail the transport-clean oracle");
});
