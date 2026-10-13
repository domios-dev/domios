#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const baselinePath = 'scripts/slow-unit-tests.json';
export const thresholdSeconds = 1;

const identity = entry => JSON.stringify([entry.binary, entry.test]);

export function parseTimings(output) {
  const text = output.replace(/\x1b\[[0-9;]*m/g, '');
  const summary = text.match(/^\s*Summary\s+\[[^\]]+\]\s+(\d+)\s+tests? run/m);
  if (!summary) throw new Error('nextest completion summary is missing');
  const entries = new Map();
  for (const match of text.matchAll(/^\s*(?:PASS|FAIL|LEAK|TIMEOUT)\s+\[\s*>?\s*([\d.]+)s\]\s+(\S+)\s+(\S+)\s*$/gm)) {
    const entry = { binary: match[2], test: match[3], measuredSeconds: Number(match[1]) };
    if (!Number.isFinite(entry.measuredSeconds)) throw new Error('invalid nextest duration');
    const previous = entries.get(identity(entry));
    if (!previous || entry.measuredSeconds > previous.measuredSeconds) entries.set(identity(entry), entry);
  }
  if (!entries.size) throw new Error('nextest report contains no per-test timings');
  if (entries.size !== Number(summary[1])) throw new Error(`incomplete nextest timings: ${entries.size} of ${summary[1]} executed tests`);
  return [...entries.values()];
}

export function validateBaseline(document) {
  if (!document || Object.keys(document).sort().join(',') !== 'entries,version' || document.version !== 1 || !Array.isArray(document.entries)) {
    throw new Error('baseline requires only version: 1 and entries');
  }
  const seen = new Set();
  for (const entry of document.entries) {
    if (!entry || Object.keys(entry).sort().join(',') !== 'binary,measuredSeconds,test' ||
        typeof entry.binary !== 'string' || !entry.binary || /\s/.test(entry.binary) ||
        typeof entry.test !== 'string' || !entry.test || /\s/.test(entry.test) ||
        !Number.isFinite(entry.measuredSeconds) || entry.measuredSeconds <= thresholdSeconds) {
      throw new Error('baseline requires exact binary/test names and a measured duration over 1 second');
    }
    if (seen.has(identity(entry))) throw new Error(`duplicate baseline: ${entry.binary} ${entry.test}`);
    seen.add(identity(entry));
  }
  return document.entries;
}

export function checkSlowTests(timings, document, previousDocument = document) {
  const entries = validateBaseline(document);
  const allowed = new Map(entries.map(entry => [identity(entry), entry]));
  const previous = new Map(validateBaseline(previousDocument).map(entry => [identity(entry), entry]));
  const errors = [];
  for (const entry of entries) {
    const original = previous.get(identity(entry));
    if (!original) errors.push(`added baseline: ${entry.binary} ${entry.test}`);
    else if (entry.measuredSeconds > original.measuredSeconds) errors.push(`increased baseline: ${entry.binary} ${entry.test}`);
  }
  for (const entry of timings.filter(entry => entry.measuredSeconds > thresholdSeconds)) {
    const allowance = allowed.get(identity(entry));
    if (!allowance) errors.push(`new slow unit: ${entry.binary} ${entry.test}: ${entry.measuredSeconds}s > 1s`);
  }
  return errors;
}

function main() {
  const args = process.argv.slice(2);
  const report = args.shift();
  if (!report || (args.length && (args.length !== 2 || args[0] !== '--base'))) {
    throw new Error('usage: node scripts/check-slow-unit-tests.mjs NEXTEST_LOG [--base REF]');
  }
  const base = args[1] ?? process.env.SLOW_UNIT_TESTS_BASE ?? 'origin/develop';
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
  const document = JSON.parse(readFileSync(resolve(root, baselinePath), 'utf8'));
  const resolved = execFileSync('git', ['rev-parse', '--verify', `${base}^{commit}`], { cwd: root, encoding: 'utf8' }).trim();
  const exists = execFileSync('git', ['ls-tree', '--name-only', resolved, '--', baselinePath], { cwd: root, encoding: 'utf8' }).trim();
  const previous = exists ? JSON.parse(execFileSync('git', ['show', `${resolved}:${baselinePath}`], { cwd: root, encoding: 'utf8' })) : document;
  const timings = parseTimings(readFileSync(report, 'utf8'));
  for (const entry of timings.filter(entry => entry.measuredSeconds > thresholdSeconds).sort((a, b) => b.measuredSeconds - a.measuredSeconds)) {
    console.log(`Slow unit: ${entry.measuredSeconds.toFixed(3)}s ${entry.binary} ${entry.test}`);
  }
  const errors = checkSlowTests(timings, document, previous);
  if (errors.length) throw new Error(errors.join('\n'));
  console.log(`Slow unit ratchet PASS: ${timings.length} measured tests, ${document.entries.length} existing slow units; baseline ${base}.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) {
    console.error(`Slow unit ratchet FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
