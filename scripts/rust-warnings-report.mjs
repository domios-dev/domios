// Caller: CI rust-warnings-summary.yml; scripts tests/collect-rust-warnings.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { deduplicate, makeBaseline, ratchet, assertBaselineShrinks, renderReport } from './collect-rust-warnings.mjs';

const directory = process.argv[2] || 'ci-rust-warnings';
const output = process.argv[3] || 'rust-warnings-report';
const baseline = JSON.parse(fs.readFileSync('scripts/rust-warnings-baseline.json', 'utf8'));
const frontendFile = 'scripts/frontend-warnings-baseline.json';
const frontendBaseline = fs.existsSync(frontendFile) ? JSON.parse(fs.readFileSync(frontendFile, 'utf8')) : null;
try {
  const git = args => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  // The fork point, not develop's head: a release cut before develop moved its baseline must not read as a reset.
  const forkPoint = git(['merge-base', 'HEAD', 'origin/develop']).trim();
  const previous = JSON.parse(git(['show', `${forkPoint}:scripts/rust-warnings-baseline.json`]));
  assertBaselineShrinks(baseline, previous);
} catch (error) {
  if (!error.status) throw error;
}
if (frontendBaseline) {
  try {
    const git = args => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    const forkPoint = git(['merge-base', 'HEAD', 'origin/develop']).trim();
    assertBaselineShrinks(frontendBaseline, JSON.parse(git(['show', `${forkPoint}:${frontendFile}`])));
  } catch (error) {
    if (!error.status) throw error;
  }
}
const files = fs.existsSync(directory) ? fs.readdirSync(directory).filter(f => /^rust-warnings-.+\.json$/.test(f) && !['rust-warnings-report.json', 'rust-warnings-baseline.json'].includes(f)) : [];
const documents = files.map(f => JSON.parse(fs.readFileSync(path.join(directory, f), 'utf8')));
const warnings = deduplicate(documents.flatMap(d => d.warnings));
const targets = [...new Set(documents.map(d => d.target))].sort();
const required = JSON.parse(process.env.WARNING_ARTIFACTS || '[]');
const missing = required.filter(name => !files.includes(name + '.json'));
const incomplete = documents.filter(d => !d.complete).map(d => d.platform + '-' + d.target);
fs.rmSync(path.join(output, 'rust-warnings-baseline.json'), { force: true });
fs.rmSync(path.join(output, 'frontend-warnings-baseline.json'), { force: true });
const combinedBaseline = frontendBaseline ? {
  ...makeBaseline([...baseline.warnings, ...frontendBaseline.warnings], [...baseline.targets, ...frontendBaseline.targets]),
  initialized: baseline.initialized && frontendBaseline.initialized,
} : baseline;
const decision = ratchet(warnings, combinedBaseline, targets);
const candidateWarnings = [...warnings, ...combinedBaseline.warnings.filter(w => !targets.includes(w.target))];
const candidateTargets = [...new Set([...targets, ...combinedBaseline.targets])];
const report = renderReport(warnings, targets, decision) + (missing.length || incomplete.length ? '\nIncomplete collection; missing or failed: ' + [...missing, ...incomplete].join(', ') + '. Baseline cannot be updated.\n' : '');
fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(output, 'rust-warnings-report.md'), report);
fs.writeFileSync(path.join(output, 'rust-warnings-report.json'), JSON.stringify({ warnings, targets, missing, incomplete, decision }, null, 2));
if (required.length && !missing.length && !incomplete.length && !decision.added.length && (baseline.initialized || process.env.WARNING_SEED === 'true')) {
  fs.writeFileSync(path.join(output, 'rust-warnings-baseline.json'), JSON.stringify(makeBaseline(candidateWarnings.filter(w => w.target !== 'frontend'), candidateTargets.filter(target => target !== 'frontend')), null, 2) + '\n');
  if (frontendBaseline) fs.writeFileSync(path.join(output, 'frontend-warnings-baseline.json'), JSON.stringify(makeBaseline(candidateWarnings.filter(w => w.target === 'frontend'), ['frontend']), null, 2) + '\n');
}
if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, report);
if (decision.added.length) process.exitCode = 1;
