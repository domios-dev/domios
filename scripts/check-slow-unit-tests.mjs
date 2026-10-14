#!/usr/bin/env node
// Caller: build.sh check|check-tests|test-one|release-preflight|install-local|build|test|verify|fmt-check; scripts check-slow-unit-tests.test.mjs, ci-rust-shards.mjs, confirm-slow-unit-tests.mjs; skill build, testing
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, appendFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const baselinePath = 'scripts/slow-unit-tests.json';
export const crateBaselinePath = 'scripts/slow-crate-unit-tests.json';
export const benchmarkPath = 'scripts/unit-test-benchmarks.json';
export const thresholdSeconds = 1;
export const newSlowFailureSeconds = 1.25;
export const regressionLimit = seconds => seconds + Math.max(seconds * 0.2, 0.25);
const identity = entry => JSON.stringify([entry.binary, entry.test]);
const exactName = /^[A-Za-z0-9_:./@-]+$/;

export function parseTimings(output) {
  const text = output.replace(/\x1b\[[0-9;]*m/g, '');
  const summaries = [...text.matchAll(/^\s*Summary\s+\[[^\]]+\]\s+(\d+)\s+tests? run/gm)];
  const executed = summaries.reduce((count, summary) => count + Number(summary[1]), 0);
  if (!summaries.length) throw new Error('nextest completion summary is missing');
  const entries = new Map();
  for (const match of text.matchAll(/^\s*(?:PASS|FAIL|LEAK|TIMEOUT)\s+\[\s*>?\s*([\d.]+)s\]\s+(?:\(\s*\d+\/\d+\)\s+)?(\S+)\s+(\S+)\s*$/gm)) {
    const entry = { binary: match[2], test: match[3], measuredSeconds: Number(match[1]) };
    if (!Number.isFinite(entry.measuredSeconds) || !exactName.test(entry.binary) || !exactName.test(entry.test)) {
      throw new Error('invalid nextest identity or duration');
    }
    const previous = entries.get(identity(entry));
    if (!previous || entry.measuredSeconds > previous.measuredSeconds) entries.set(identity(entry), entry);
  }
  if (!entries.size) throw new Error('nextest report contains no per-test timings');
  if (entries.size !== executed) throw new Error(`incomplete nextest timings: ${entries.size} of ${executed} executed tests`);
  return [...entries.values()];
}

export function validateBaseline(document) {
  if (!document || Object.keys(document).sort().join(',') !== 'entries,version' || document.version !== 1 || !Array.isArray(document.entries)) {
    throw new Error('baseline requires only version: 1 and entries');
  }
  const seen = new Set();
  for (const entry of document.entries) {
    if (!entry || Object.keys(entry).sort().join(',') !== 'binary,measuredSeconds,test' ||
        typeof entry.binary !== 'string' || !exactName.test(entry.binary) ||
        typeof entry.test !== 'string' || !exactName.test(entry.test) ||
        !Number.isFinite(entry.measuredSeconds) || entry.measuredSeconds <= thresholdSeconds) {
      throw new Error('baseline requires exact binary/test names and a measured duration over 1 second');
    }
    if (seen.has(identity(entry))) throw new Error(`duplicate baseline: ${entry.binary} ${entry.test}`);
    seen.add(identity(entry));
  }
  return document.entries;
}

export function checkBaseline(document, previousDocument) {
  const entries = validateBaseline(document);
  const previous = new Map(validateBaseline(previousDocument).map(entry => [identity(entry), entry]));
  return entries.flatMap(entry => {
    const original = previous.get(identity(entry));
    if (!original) return [`added baseline: ${entry.binary} ${entry.test}`];
    if (entry.measuredSeconds > original.measuredSeconds) return [`increased baseline: ${entry.binary} ${entry.test}`];
    return [];
  });
}

export function validateBenchmarks(document) {
  if (!document || Object.keys(document).sort().join(',') !== 'entries,version' || document.version !== 1 || !Array.isArray(document.entries)) {
    throw new Error('benchmark exemptions require only version: 1 and entries');
  }
  const seen = new Set();
  for (const entry of document.entries) {
    if (!entry || Object.keys(entry).sort().join(',') !== 'binary,reason,test' ||
        typeof entry.binary !== 'string' || !exactName.test(entry.binary) ||
        typeof entry.test !== 'string' || !exactName.test(entry.test) ||
        typeof entry.reason !== 'string' || !entry.reason.trim() || seen.has(identity(entry))) {
      throw new Error('benchmark exemption requires a unique exact identity and explicit reason');
    }
    seen.add(identity(entry));
  }
  return document.entries;
}

export function checkBenchmarks(document, previousDocument) {
  const previous = new Map(validateBenchmarks(previousDocument).map(entry => [identity(entry), entry.reason]));
  return validateBenchmarks(document).flatMap(entry => previous.get(identity(entry)) === entry.reason
    ? [] : [`added or changed benchmark exemption: ${entry.binary} ${entry.test}`]);
}

export function selectCandidates(timings, document, benchmarks = []) {
  const allowed = new Map(validateBaseline(document).map(entry => [identity(entry), entry.measuredSeconds]));
  const exempt = new Set(benchmarks.map(identity));
  return timings.filter(entry => !exempt.has(identity(entry)) && entry.measuredSeconds > (allowed.get(identity(entry)) ?? thresholdSeconds));
}

export function candidateFilter(entries) {
  return entries.map(entry => {
    if (!exactName.test(entry.binary) || !exactName.test(entry.test)) throw new Error('unsafe candidate identity');
    return `(binary_id(=${entry.binary}) & test(=${entry.test}))`;
  }).join(' | ');
}

export function slowTestVerdict(timings, document, previousDocument = document, alone = [], benchmarks = []) {
  const regressions = [];
  const errors = checkBaseline(document, previousDocument);
  const allowed = new Map(validateBaseline(document).map(entry => [identity(entry), entry.measuredSeconds]));
  const measuredAlone = new Map(alone.map(entry => [identity(entry), entry.measuredSeconds]));
  const exempt = new Set(benchmarks.map(identity));
  for (const entry of document.entries) {
    if (exempt.has(identity(entry))) errors.push(`benchmark must not be baselined: ${entry.binary} ${entry.test}`);
  }
  for (const candidate of selectCandidates(timings, document, benchmarks)) {
    const seconds = measuredAlone.get(identity(candidate));
    if (seconds === undefined) {
      errors.push(`missing alone timing: ${candidate.binary} ${candidate.test}`);
    } else if (seconds > (allowed.has(identity(candidate))
      ? regressionLimit(allowed.get(identity(candidate))) : newSlowFailureSeconds)) {
      const kind = allowed.has(identity(candidate)) ? 'slow unit regression' : 'new slow unit';
      regressions.push(`${kind}: ${candidate.binary} ${candidate.test}: parallel=${candidate.measuredSeconds}s alone=${seconds}s`);
    }
  }
  return { errors, regressions };
}

export function checkSlowTests(...args) {
  const { errors, regressions } = slowTestVerdict(...args);
  return [...errors, ...regressions];
}

export function timingWarnings(alone, document) {
  const allowed = new Map(validateBaseline(document).map(entry => [identity(entry), entry.measuredSeconds]));
  return alone.flatMap(entry => {
    const baseline = allowed.get(identity(entry));
    const withinNewBand = entry.measuredSeconds > thresholdSeconds && entry.measuredSeconds <= newSlowFailureSeconds;
    const withinRegressionBand = baseline !== undefined && entry.measuredSeconds > baseline && entry.measuredSeconds <= regressionLimit(baseline);
    return withinNewBand || withinRegressionBand
      ? [`Unit timing WARNING: alone=${entry.measuredSeconds.toFixed(3)}s ${entry.binary} ${entry.test}; 1s unit target, within measurement noise band.`] : [];
  });
}

function main() {
  const args = process.argv.slice(2);
  const report = args.shift();
  const options = { base: process.env.SLOW_UNIT_TESTS_BASE ?? 'origin/develop', policy: 'enforce' };
  if (!report) throw new Error('nextest log is required');
  while (args.length) {
    const flag = args.shift();
    const value = args.shift();
    if (!['--base', '--candidates', '--alone', '--policy'].includes(flag) || !value || value.startsWith('--')) {
      throw new Error('usage: node scripts/check-slow-unit-tests.mjs LOG [--base REF] [--candidates JSON | --alone LOG] [--policy enforce|advisory]');
    }
    const key = flag.slice(2);
    if (!['base', 'policy'].includes(key) && options[key]) throw new Error(`duplicate ${flag}`);
    options[key] = value;
  }
  if (!['enforce', 'advisory'].includes(options.policy)) throw new Error('Invalid timing policy');
  if (options.candidates && options.alone) throw new Error('--candidates and --alone are mutually exclusive');
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
  const resolved = execFileSync('git', ['rev-parse', '--verify', `${options.base}^{commit}`], { cwd: root, encoding: 'utf8' }).trim();
  const load = (path, optional = false) => {
    let current;
    try { current = JSON.parse(readFileSync(resolve(root, path), 'utf8')); }
    catch (error) { if (!optional || error.code !== 'ENOENT') throw error; current = {version: 1, entries: []}; }
    const exists = execFileSync('git', ['ls-tree', '--name-only', resolved, '--', path], {cwd: root, encoding: 'utf8'}).trim();
    const previous = exists ? JSON.parse(execFileSync('git', ['show', `${resolved}:${path}`], {cwd: root, encoding: 'utf8'})) : current;
    return {current, previous};
  };
  const app = load(baselinePath);
  const crates = load(crateBaselinePath, true);
  const exemption = load(benchmarkPath, true);
  const benchmarkErrors = checkBenchmarks(exemption.current, exemption.previous);
  if (benchmarkErrors.length) throw new Error(benchmarkErrors.join('\n'));
  const benchmarks = validateBenchmarks(exemption.current);
  const document = {version: 1, entries: [...validateBaseline(app.current), ...validateBaseline(crates.current)]};
  const previous = {version: 1, entries: [...validateBaseline(app.previous), ...validateBaseline(crates.previous)]};
  const timings = parseTimings(readFileSync(report, 'utf8'));
  const candidates = selectCandidates(timings, document, benchmarks);
  const timed = new Map(timings.map(entry => [identity(entry), entry.measuredSeconds]));
  for (const entry of benchmarks) {
    const seconds = timed.get(identity(entry));
    if (seconds !== undefined) console.log(`Unit benchmark EXEMPT: parallel=${seconds.toFixed(3)}s ${entry.binary} ${entry.test}; ${entry.reason}`);
  }
  if (options.candidates) {
    const errors = checkBaseline(document, previous);
    if (errors.length) throw new Error(errors.join('\n'));
    writeFileSync(options.candidates, JSON.stringify({
      entries: candidates.map(({binary, test}) => ({binary, test})),
      filter: candidateFilter(candidates),
    }, null, 2) + '\n');
    console.log(`Slow unit candidates: ${candidates.length} of ${timings.length} tests require serial measurement.`);
    return;
  }
  const alone = options.alone ? parseTimings(readFileSync(options.alone, 'utf8')) : [];
  const measuredAlone = new Map(alone.map(entry => [identity(entry), entry.measuredSeconds]));
  for (const candidate of candidates) {
    console.log(`Unit timing: parallel=${candidate.measuredSeconds.toFixed(3)}s alone=${measuredAlone.get(identity(candidate))?.toFixed(3) ?? 'missing'}s ${candidate.binary} ${candidate.test}`);
  }
  console.log("Unit timing rule: 1s target; new alone >1.25s fails; regression above baseline + max(20%, 0.25s) fails.");
  for (const warning of timingWarnings(alone, document)) console.log(warning);
  const { errors, regressions } = slowTestVerdict(timings, document, previous, alone, benchmarks);
  if (errors.length) throw new Error(errors.join('\n'));
  if (regressions.length) {
    if (options.policy === 'enforce') throw new Error(regressions.join('\n'));
    const warning = `::warning::Slow unit ratchet ADVISORY: ${regressions.join('; ')}`;
    console.log(warning);
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n${warning}\n`);
    return;
  }
  console.log(`Slow unit ratchet PASS: ${timings.length} measured tests, ${candidates.length} serial confirmations, ${document.entries.length} existing slow units; baseline ${options.base}.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) {
    console.error(`Slow unit ratchet FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
