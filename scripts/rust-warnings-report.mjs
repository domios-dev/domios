import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { deduplicate, makeBaseline, ratchet, assertBaselineShrinks, renderReport } from './collect-rust-warnings.mjs';

const directory = process.argv[2] || 'ci-rust-warnings';
const output = process.argv[3] || 'rust-warnings-report';
const baseline = JSON.parse(fs.readFileSync('scripts/rust-warnings-baseline.json', 'utf8'));
try {
  const previous = JSON.parse(execFileSync('git', ['show', 'origin/develop:scripts/rust-warnings-baseline.json'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }));
  assertBaselineShrinks(baseline, previous);
} catch (error) {
  if (!error.status) throw error;
}
const files = fs.existsSync(directory) ? fs.readdirSync(directory).filter(f => /^rust-warnings-.+\.json$/.test(f) && !['rust-warnings-report.json', 'rust-warnings-baseline.json'].includes(f)) : [];
const documents = files.map(f => JSON.parse(fs.readFileSync(path.join(directory, f), 'utf8')));
const warnings = deduplicate(documents.flatMap(d => d.warnings));
const targets = [...new Set(documents.map(d => d.target))].sort();
const required = JSON.parse(process.env.WARNING_ARTIFACTS || '[]');
const missing = required.filter(name => !files.includes(name + '.json'));
const incomplete = documents.filter(d => !d.complete).map(d => d.platform + '-' + d.target);
fs.rmSync(path.join(output, 'rust-warnings-baseline.json'), { force: true });
const decision = ratchet(warnings, baseline, targets);
const candidateWarnings = [...warnings, ...baseline.warnings.filter(w => !targets.includes(w.target))];
const candidateTargets = [...new Set([...targets, ...baseline.targets])];
const report = renderReport(warnings, targets, decision) + (missing.length || incomplete.length ? '\nIncomplete collection; missing or failed: ' + [...missing, ...incomplete].join(', ') + '. Baseline cannot be updated.\n' : '');
fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(output, 'rust-warnings-report.md'), report);
fs.writeFileSync(path.join(output, 'rust-warnings-report.json'), JSON.stringify({ warnings, targets, missing, incomplete, decision }, null, 2));
if (required.length && !missing.length && !incomplete.length && !decision.added.length && (baseline.initialized || process.env.WARNING_SEED === 'true')) {
  fs.writeFileSync(path.join(output, 'rust-warnings-baseline.json'), JSON.stringify(makeBaseline(candidateWarnings, candidateTargets), null, 2) + '\n');
}
if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, report);
if (decision.added.length) process.exitCode = 1;
