#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const baselinePath = 'scripts/slow-unit-tests.json';
export const thresholdSeconds = 1;
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

export function selectCandidates(timings, document) {
  const allowed = new Map(validateBaseline(document).map(entry => [identity(entry), entry.measuredSeconds]));
  return timings.filter(entry => entry.measuredSeconds > (allowed.get(identity(entry)) ?? thresholdSeconds));
}

export function candidateFilter(entries) {
  return entries.map(entry => {
    if (!exactName.test(entry.binary) || !exactName.test(entry.test)) throw new Error('unsafe candidate identity');
    return `(binary_id(=${entry.binary}) & test(=${entry.test}))`;
  }).join(' | ');
}

export function checkSlowTests(timings, document, previousDocument = document, alone = []) {
  const errors = checkBaseline(document, previousDocument);
  const allowed = new Map(validateBaseline(document).map(entry => [identity(entry), entry.measuredSeconds]));
  const measuredAlone = new Map(alone.map(entry => [identity(entry), entry.measuredSeconds]));
  for (const candidate of selectCandidates(timings, document)) {
    const seconds = measuredAlone.get(identity(candidate));
    if (seconds === undefined) {
      errors.push(`missing alone timing: ${candidate.binary} ${candidate.test}`);
    } else if (seconds > (allowed.get(identity(candidate)) ?? thresholdSeconds)) {
      const kind = allowed.has(identity(candidate)) ? 'slow unit regression' : 'new slow unit';
      errors.push(`${kind}: ${candidate.binary} ${candidate.test}: parallel=${candidate.measuredSeconds}s alone=${seconds}s`);
    }
  }
  return errors;
}

function main() {
  const args = process.argv.slice(2);
  const report = args.shift();
  const options = { base: process.env.SLOW_UNIT_TESTS_BASE ?? 'origin/develop' };
  if (!report) throw new Error('nextest log is required');
  while (args.length) {
    const flag = args.shift();
    const value = args.shift();
    if (!['--base', '--candidates', '--alone'].includes(flag) || !value || value.startsWith('--')) {
      throw new Error('usage: node scripts/check-slow-unit-tests.mjs LOG [--base REF] [--candidates JSON | --alone LOG]');
    }
    const key = flag.slice(2);
    if (key !== 'base' && options[key]) throw new Error(`duplicate ${flag}`);
    options[key] = value;
  }
  if (options.candidates && options.alone) throw new Error('--candidates and --alone are mutually exclusive');
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
  const document = JSON.parse(readFileSync(resolve(root, baselinePath), 'utf8'));
  const resolved = execFileSync('git', ['rev-parse', '--verify', `${options.base}^{commit}`], { cwd: root, encoding: 'utf8' }).trim();
  const exists = execFileSync('git', ['ls-tree', '--name-only', resolved, '--', baselinePath], { cwd: root, encoding: 'utf8' }).trim();
  const previous = exists ? JSON.parse(execFileSync('git', ['show', `${resolved}:${baselinePath}`], { cwd: root, encoding: 'utf8' })) : document;
  const timings = parseTimings(readFileSync(report, 'utf8'));
  const candidates = selectCandidates(timings, document);
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
  const errors = checkSlowTests(timings, document, previous, alone);
  if (errors.length) throw new Error(errors.join('\n'));
  console.log(`Slow unit ratchet PASS: ${timings.length} measured tests, ${candidates.length} serial confirmations, ${document.entries.length} existing slow units; baseline ${options.base}.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) {
    console.error(`Slow unit ratchet FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
