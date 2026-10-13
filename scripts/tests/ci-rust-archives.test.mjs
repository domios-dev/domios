import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { archivePlatform, archiveRunArgs, validateArchiveSuite, verifyArchiveBuilds } from '../ci-rust-archives.mjs';

const builds = () => ['macOS', 'Linux', 'Windows'].map((os) => ({ os, arch: os === 'macOS' ? 'ARM64' : 'X64', sha: 'TOKEN_SHA', exit: 0, runner: os + '-builder', seconds: 10 }));
const receipts = () => builds().map(({ os, arch, sha }) => ({ os, arch, sha }));
test('archive identity binds operating system and architecture', () => {
  assert.equal(archivePlatform('macOS', 'ARM64'), 'macOS-ARM64');
  assert.throws(() => archivePlatform('Linux', 'unknown'));
});
test('classified archive inventory must match source OS and architecture', () => {
  const document = { os: 'Windows', arch: 'X64', sha: 'TOKEN_SHA', rows: [{ id: 'test' }] };
  assert.deepEqual(validateArchiveSuite(document, 'Windows', 'X64', 'TOKEN_SHA'), document.rows);
  for (const expected of [['Linux', 'X64', 'TOKEN_SHA'], ['Windows', 'ARM64', 'TOKEN_SHA'], ['Windows', 'X64', 'OTHER_SHA']]) assert.throws(() => validateArchiveSuite(document, ...expected));
  assert.throws(() => validateArchiveSuite({ ...document, rows: [] }, 'Windows', 'X64', 'TOKEN_SHA'));
});
test('all consumers reference one successful archive build per OS and architecture', () => {
  assert.match(verifyArchiveBuilds(builds(), receipts()), /macOS-builder.*10.0 s/);
  const failed = builds(); failed[0].exit = 1;
  assert.throws(() => verifyArchiveBuilds(failed, receipts()), /failed/);
  assert.throws(() => verifyArchiveBuilds(builds().slice(1), receipts()), /Missing/);
  assert.throws(() => verifyArchiveBuilds([...builds(), builds()[0]], receipts()), /Duplicate/);
  const wrong = receipts(); wrong[0].sha = 'OTHER_SHA';
  assert.throws(() => verifyArchiveBuilds(builds(), wrong), /matching/);
});
test('archive runs contain remapping and partition runtime controls without compiler options', () => {
  const args = archiveRunArgs('ARCHIVE', 'SOURCE', 'EXTRACT', 'CONFIG', ['--partition', 'hash:1/12']);
  assert.equal(args[args.indexOf('--archive-file') + 1], 'ARCHIVE');
  assert.equal(args[args.indexOf('--workspace-remap') + 1], 'SOURCE');
  assert.ok(!args.some((arg) => ['--manifest-path', '--lib', '--workspace', '--features', '--locked'].includes(arg)));
  assert.ok(!args.includes('--test-threads'));
  assert.equal(archiveRunArgs('ARCHIVE', 'SOURCE', 'EXTRACT', 'CONFIG', [], true).at(-1), '1');
});
test('only archive builders install Rust; free consumers choose their platform artifact', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/tests.yml', import.meta.url), 'utf8');
  const consumers = workflow.slice(workflow.indexOf('  rust-shards:'), workflow.indexOf('  rust-coverage:'));
  assert.doesNotMatch(consumers, /rust-toolchain|Prepare compile fixtures/);
  assert.match(consumers, /rust-archive-\$\{\{ runner.os \}\}-\$\{\{ runner.arch \}\}/);
  assert.match(consumers, /runs-on: \[self-hosted, rust-shard\]/);
});
