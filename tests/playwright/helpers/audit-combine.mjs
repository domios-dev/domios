import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runStaticAudit } from "./audit-static.mjs";
import { runTraceAudit } from "./audit-trace.mjs";

const helpersDir = fileURLToPath(new URL(".", import.meta.url));
const auditPath = resolve(helpersDir, "../AUDIT.md");

function verdictForTest(test, findings) {
  const own = findings.filter((finding) => finding.file === test.file && finding.testTitle === test.title);
  if (own.some((finding) => finding.severity === "error")) return "weak";
  if (own.length > 0) return "watch";
  return "good";
}

function tableCell(value) {
  return String(value ?? "").replace(/\|/g, "\\|").replace(/\n/g, " ");
}

function renderStaticSection(staticAudit) {
  const lines = [
    "## Static Review",
    "",
    `Files: ${staticAudit.summary.files}. Tests: ${staticAudit.summary.tests}. Findings: ${staticAudit.summary.findings} (${staticAudit.summary.errors} errors, ${staticAudit.summary.warnings} warnings).`,
    "",
    "| Verdict | Test | File | Findings |",
    "| --- | --- | --- | ---: |",
  ];

  for (const test of staticAudit.tests) {
    lines.push(
      `| ${verdictForTest(test, staticAudit.findings)} | ${tableCell(test.title)} | ${tableCell(`${test.file}:${test.line}`)} | ${test.findings} |`,
    );
  }

  lines.push("", "### Findings", "");
  if (staticAudit.findings.length === 0) {
    lines.push("No static findings.");
  } else {
    lines.push("| Severity | Rule | Location | Test | Message |", "| --- | --- | --- | --- | --- |");
    for (const finding of staticAudit.findings) {
      lines.push(
        `| ${finding.severity} | ${finding.rule} | ${tableCell(`${finding.file}:${finding.line}`)} | ${tableCell(finding.testTitle ?? "-")} | ${tableCell(finding.message)} |`,
      );
    }
  }
  return lines;
}

function renderTraceSection(traceAudit) {
  const lines = [
    "## Trace Review",
    "",
    `Trace ZIPs: ${traceAudit.summary.traces}. Parsed records: ${traceAudit.summary.records}. Errors: ${traceAudit.summary.errors}.`,
    "",
  ];

  if (traceAudit.records.length === 0) {
    lines.push("No trace artifacts found under `tests/playwright/artifacts`.");
  } else {
    lines.push(
      "| Verdict | Score | Test | Actions | Assertions | Waits | Sleeps | Screens | Trace |",
      "| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |",
    );
    for (const record of traceAudit.records) {
      const screens = record.screenshots + record.screencastFrames;
      lines.push(
        `| ${record.verdict} | ${record.score} | ${tableCell(record.title)} | ${record.actionCount} | ${record.assertionCount} | ${record.realWaitCount} | ${record.sleepCount} | ${screens} | [trace.zip](${record.tracePath}) |`,
      );
    }
  }

  if (traceAudit.errors.length > 0) {
    lines.push("", "### Trace Parse Errors", "");
    lines.push("| Trace | Error |", "| --- | --- |");
    for (const error of traceAudit.errors) {
      lines.push(`| ${tableCell(error.tracePath)} | ${tableCell(error.message)} |`);
    }
  }

  return lines;
}

export function writeAuditMarkdown(options = {}) {
  const staticAudit = runStaticAudit();
  const traceAudit = runTraceAudit({ inputs: options.traceInputs ?? [] });
  const lines = [
    "# Playwright Test Audit",
    "",
    `Generated: ${new Date().toISOString()}`,
    "",
    "Human replay: open linked traces with `pnpm test:pw:trace <trace.zip>` or `npx playwright show-trace <trace.zip>`.",
    "",
    ...renderStaticSection(staticAudit),
    "",
    ...renderTraceSection(traceAudit),
    "",
  ];

  mkdirSync(dirname(auditPath), { recursive: true });
  writeFileSync(auditPath, `${lines.join("\n")}\n`);
  return {
    auditPath,
    static: staticAudit.summary,
    trace: traceAudit.summary,
  };
}

const result = writeAuditMarkdown({ traceInputs: process.argv.slice(2) });
process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
