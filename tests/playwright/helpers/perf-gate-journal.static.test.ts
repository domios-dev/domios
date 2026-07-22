import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const source = readFileSync(join(process.cwd(), "tests/playwright/suites/00-perf-gate/perf-gate.spec.ts"), "utf8");

function renderMetricUsesTerminatedRawSend(source: string): boolean {
  return /await api\.post\(\s*`\/panes\/\$\{paneId\}\/send`,\s*\{\s*text: `STUB_ECHO \$\{marker\}\\r`,\s*raw: true\s*\}\s*\)/s.test(source);
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

test("render metric terminates its transport-clean raw input", () => {
  assert.ok(renderMetricUsesTerminatedRawSend(source), "render metric raw send must include CR");
  const unterminated = source.replace(
    "{ text: `STUB_ECHO ${marker}\\r`, raw: true }",
    "{ text: `STUB_ECHO ${marker}`, raw: true }",
  );
  assert.notEqual(unterminated, source, "mutation must remove the raw-input terminator");
  assert.equal(renderMetricUsesTerminatedRawSend(unterminated), false, "unterminated raw bytes must fail the oracle");
});

test("terminal focus scopes duplicate titles by canonical tab identity", () => {
  assert.match(source, /\[data-testid="tab-item"\]\[data-tab-id="\$\{pane\.tab_id\}"\]/);
  assert.match(source, /await api\.focusPane\(paneId\)/);
  assert.match(source, /toHaveAttribute\("aria-busy", "false"/);
  assert.doesNotMatch(source, /page\.getByRole\("button", \{ name: `Open tab \$\{pane\.title\}`/);
});
