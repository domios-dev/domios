#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const thresholdSeconds = 1;
const exactName = /^[A-Za-z0-9_:./@-]+$/;
const runIdentity = entry => JSON.stringify([entry.binary, entry.test]);

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
    const previous = entries.get(runIdentity(entry));
    if (!previous || entry.measuredSeconds > previous.measuredSeconds) entries.set(runIdentity(entry), entry);
  }
  // A runner whose inventory holds no test passes nextest's no-tests check and has nothing to measure.
  if (!entries.size && executed > 0) throw new Error('nextest report contains no per-test timings');
  if (entries.size !== executed) throw new Error(`incomplete nextest timings: ${entries.size} of ${executed} executed tests`);
  return [...entries.values()];
}

export function candidateFilter(entries) {
  return entries.map(entry => {
    if (!exactName.test(entry.binary) || !exactName.test(entry.test)) throw new Error('unsafe candidate identity');
    return `(binary_id(=${entry.binary}) & test(=${entry.test}))`;
  }).join(' | ');
}


function main() {
  const [report, ...args] = process.argv.slice(2);
  if (!report) throw new Error('nextest log is required');
  const options = {};
  while (args.length) {
    const flag = args.shift();
    if (flag === '--serial') { options.serial = true; continue; }
    if (!['--candidates', '--alone'].includes(flag) || !args[0] || args[0].startsWith('--')) {
      throw new Error('usage: node scripts/check-slow-unit-tests.mjs LOG [--serial | --candidates JSON | --alone LOG]');
    }
    const key = flag.slice(2);
    if (options[key]) throw new Error(`duplicate ${flag}`);
    options[key] = args.shift();
  }
  if (options.candidates && options.alone) throw new Error('--candidates and --alone are mutually exclusive');
  const timings = parseTimings(readFileSync(report, 'utf8'));
  const candidates = timings.filter(entry => entry.measuredSeconds >= thresholdSeconds);
  if (options.candidates) {
    writeFileSync(options.candidates, JSON.stringify({
      entries: candidates.map(({ binary, test }) => ({ binary, test })),
      filter: candidateFilter(candidates),
    }, null, 2) + '\n');
  }
  const alone = options.alone ? parseTimings(readFileSync(options.alone, 'utf8')) : [];
  const confirmed = new Map(alone.map(entry => [runIdentity(entry), entry.measuredSeconds]));
  for (const entry of candidates) {
    const seconds = confirmed.get(runIdentity(entry));
    console.log(`::warning::Unit timing ${options.serial ? 'serial' : 'parallel'}=${entry.measuredSeconds.toFixed(3)}s${seconds === undefined ? '' : ` alone=${seconds.toFixed(3)}s`} ${entry.binary} ${entry.test}`);
  }
  console.log(`Unit timing ADVISORY: ${timings.length} measured tests, ${candidates.length} at or above 1s; slowness never fails or terminates a test.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) {
    console.error(`Unit timing evidence FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
