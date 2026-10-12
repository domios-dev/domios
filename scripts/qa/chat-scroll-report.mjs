import { readFile } from 'node:fs/promises';
import { paintedGapSamples } from './chat-scroll-criteria.mjs';

const [reportPath, summaryPath] = process.argv.slice(2);
if (!reportPath || !summaryPath) throw new Error('Usage: --check-report REPORT_JSON CRITERIA_SUMMARY_JSON');
const report = JSON.parse(await readFile(reportPath, 'utf8'));
const summary = JSON.parse(await readFile(summaryPath, 'utf8'));
const caseLabels = new Set(report.cases.map((result) => result.label));
const globalErrors = (report.failures ?? []).filter((failure) => !caseLabels.has(failure));
if (report.error) globalErrors.push(report.error);
let failures = globalErrors.length;
if (globalErrors.length) console.log(JSON.stringify({ globalErrors }));
for (const result of report.cases) {
  const reference = summary.cases.find((entry) => entry.label === result.label);
  const gaps = paintedGapSamples(result.samples, { afterSend: result.phase !== undefined });
  const structuralErrors = result.errors.filter((error) => !/^\d+ painted samples outside bottom$/.test(error));
  const verdict = gaps.length || structuralErrors.length ? 'FAIL' : 'PASS';
  const agrees = reference?.verdict === verdict && (reference.maxPaintedGap > 1) === (gaps.length > 0);
  console.log(JSON.stringify({ label: result.label, verdict, paintedGaps: gaps.map((sample) => sample.gap), structuralErrors, agrees }));
  if (verdict !== 'PASS' || !agrees) failures += 1;
}
if (!report.cases.length || report.cases.length !== summary.cases.length) failures += 1;
console.log(`Offline cases=${report.cases.length}; failures=${failures}`);
process.exitCode = failures ? 1 : 0;
