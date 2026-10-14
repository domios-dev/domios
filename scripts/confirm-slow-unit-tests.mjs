#!/usr/bin/env node
// Caller: build.sh check|check-tests|test-one|release-preflight|install-local|build|test|verify|fmt-check; scripts check-slow-unit-tests.test.mjs
import { spawnSync } from 'node:child_process';
import { readFileSync, appendFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateFilter, parseTimings } from './check-slow-unit-tests.mjs';

export function confirmCandidates(entries, options, run = spawnSync) {
  candidateFilter(entries);
  const identities = entries.map(entry => JSON.stringify([entry.binary, entry.test]));
  if (new Set(identities).size !== identities.length) throw new Error('duplicate serial candidate');
  writeFileSync(options.output, '');
  let exitCode = 0;
  for (const entry of entries) {
    const filter = candidateFilter([entry]);
    const reuse = options.archive ? ['--archive-file', options.archive, '--workspace-remap', process.cwd()] : ['--binaries-metadata', options.binaries, '--cargo-metadata', options.cargo];
    const result = run('bash', ['scripts/test-capped.sh', ...reuse,
      '--filter-expr', filter, '--test-threads', '1', '--status-level', 'pass',
      '--no-fail-fast', '--no-tests=fail'], {
      encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
      env: {...process.env, CARGO_TEST_CMD: 'cargo nextest run', CARGO_TEST_BUILD_CMD: '', TEST_RAW_OUTPUT: '1'},
    });
    const output = (result.stdout ?? '') + (result.stderr ?? '');
    appendFileSync(options.output, output);
    if (result.error) throw result.error;
    const timings = parseTimings(output);
    if (timings.length !== 1 || timings[0].binary !== entry.binary || timings[0].test !== entry.test) {
      throw new Error('serial measurement selected a different test');
    }
    if (result.status !== 0) (options.print ?? (text => process.stdout.write(text)))(output.split('\n').slice(-25).join('\n'));
    if (result.status !== 0) exitCode ||= result.status ?? 1;
  }
  (options.print ?? (text => process.stdout.write(text)))(`slow-unit confirmation: selectors=${entries.length} exit=${exitCode} artifact=${options.output}\n`);
  return exitCode;
}

function main() {
  const [candidates, binaries, cargo, output, ...extra] = process.argv.slice(2);
  if (!output || extra.length) throw new Error('usage: node scripts/confirm-slow-unit-tests.mjs CANDIDATES_JSON BINARIES_JSON CARGO_JSON ALONE_LOG');
  const document = JSON.parse(readFileSync(candidates, 'utf8'));
  if (!Array.isArray(document.entries)) throw new Error('candidate entries are required');
  const reuse = binaries === '--archive-file' ? { archive: cargo } : { binaries, cargo };
  process.exitCode = confirmCandidates(document.entries, {...reuse, output});
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) {
    console.error(`Slow unit serial confirmation FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}
