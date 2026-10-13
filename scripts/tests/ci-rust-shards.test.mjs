import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { classifyInventory, filterFor, verifyCoverage } from '../ci-rust-shards.mjs';

const row = (name, platform = false) => ({ id: `crate::${name}`, name, platform, binary: 'crate', package: 'crate' });
function fixture() {
  const all = [row('shared_a'), row('shared_b'), row('native', true)];
  return [
    ...['Linux', 'macOS', 'Windows'].map((os) => ({ mode: 'platform', runner: os, os, sha: 'abc', exit: 0, seconds: 1, all: structuredClone(all), selected: ['crate::native'] })),
    ...[1, 2].map((shard) => ({ mode: 'shared', shard, runner: `runner-${shard}`, os: 'Linux', sha: 'abc', exit: 0, seconds: 2, all: structuredClone(all), selected: [`crate::shared_${shard === 1 ? 'a' : 'b'}`] })),
  ];
}

test('complete coverage reports runners and durations', () => {
  assert.match(verifyCoverage(fixture(), 2), /shard 2\/2.*runner-2.*2.0 s/);
});
for (const [name, mutate] of [
  ['missing receipt', (r) => r.pop()],
  ['duplicate shard', (r) => { r[4].shard = 1; }],
  ['invalid shard', (r) => { r[4].shard = 3; }],
  ['wrong OS', (r) => { r[2].os = 'Linux'; }],
  ['mixed source', (r) => { r[0].sha = 'def'; }],
  ['failed job', (r) => { r[0].exit = 1; }],
  ['omitted shared test', (r) => { r[4].selected = []; }],
  ['duplicate shared test', (r) => { r[4].selected = r[3].selected; }],
  ['platform test in shard', (r) => { r[4].selected.push('crate::native'); }],
  ['omitted platform test', (r) => { r[0].selected = []; }],
  ['OS-specific test misclassified as shared', (r) => { r[1].all = [row('shared_a'), row('native', true)]; }],
]) test(`coverage refuses ${name}`, () => {
  const receipts = fixture();
  mutate(receipts);
  assert.throws(() => verifyCoverage(receipts, 2));
});

function inventory(ignored = false, status = 'listed') {
  return { 'rust-suites': { crate: { 'binary-id': 'crate', 'binary-name': 'crate', 'package-name': 'crate', status, testcases: { 'tests::a': { ignored } } } } };
}
test('classification joins compiled names and inherited platform gates', () => {
  assert.equal(classifyInventory(inventory(), { crate: [{ name: 'tests::a', platform: true }] })[0].platform, true);
});
test('classification refuses unmapped and ignored compiled tests', () => {
  assert.throws(() => classifyInventory(inventory(), { crate: [] }), /Unclassified/);
  assert.throws(() => classifyInventory(inventory(true), { crate: [{ name: 'tests::a' }] }), /Ignored/);
  assert.throws(() => classifyInventory(inventory(false, 'skipped'), {}), /Unlisted/);
});
test('parameterized cases inherit the declared function gate', () => {
  const input = inventory();
  input['rust-suites'].crate.testcases = { 'tests::a::case_1': { ignored: false } };
  assert.equal(classifyInventory(input, { crate: [{ name: 'tests::a', platform: true }] })[0].platform, true);
});
test('filters complement native tests within their Cargo binary', () => {
  const rows = [row('tests::native', true), row('tests::shared')];
  assert.match(filterFor(rows, 'shared'), /not \(test\(/);
  assert.doesNotMatch(filterFor(rows, 'platform'), /not/);
  assert.match(filterFor(rows, 'shared'), /package\(=crate\) & binary\(=crate\)/);
  assert.match(filterFor([row('only_shared')], 'platform'), /none\(\)/);
});
test('workflow shards use a shared pool and all results feed the coverage job', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/tests.yml', import.meta.url), 'utf8');
  assert.match(workflow, /runs-on: \[self-hosted, rust-shard\]/);
  assert.match(workflow, /shard: \[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12\]/);
  assert.match(workflow, /needs: \[changed, rust-shards, rust-platform\]/);
  assert.match(workflow, /fail-fast: false/);
});

test('explicit ignored reasons remain visible and cannot count as executed coverage', () => {
  const source = { crate: [{ name: 'tests::a', platform: true, ignore_reason: 'TOKEN_REASON' }] };
  const rows = classifyInventory(inventory(true), source);
  assert.equal(rows[0].reason, 'TOKEN_REASON');
  assert.equal(rows[0].ignored, true);
  assert.match(filterFor(rows, 'platform'), /none\(\)/);
  const receipts = fixture();
  receipts[0].all.push({ ...row('explicit_ignore', true), ignored: true, reason: 'TOKEN_REASON' });
  assert.match(verifyCoverage(receipts, 2), /TOKEN_REASON: 1 distinct tests, 1 OS\/test entries/);
  receipts[0].all.at(-1).reason = '';
  assert.throws(() => verifyCoverage(receipts, 2), /Unexplained/);
});

test('listener isolation preserves every selected test and omits only empty groups', async () => {
  const { executionGroups } = await import('../ci-rust-shards.mjs');
  const normal = { name: 'tests::normal' };
  const listener = { name: 'startup::rest_api::tests::rebind' };
  assert.deepEqual(executionGroups([]), []);
  assert.deepEqual(executionGroups([normal]), [{ listener: false, selected: [normal] }]);
  assert.deepEqual(executionGroups([listener]), [{ listener: true, selected: [listener] }]);
  assert.deepEqual(executionGroups([normal, listener]).flatMap((g) => g.selected), [normal, listener]);
});

test('captured nextest logs settle both streams and retain failing process status', async () => {
  const { captureCommand } = await import('../ci-rust-shards.mjs');
  const { mkdtempSync, readFileSync, rmSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const directory = mkdtempSync(join(tmpdir(), 'ci-shard-capture-'));
  try {
    const log = join(directory, 'nextest.log');
    const result = await captureCommand(process.execPath, ['-e', 'process.stdout.write("TOKEN_PASS\\n"); process.stderr.write("TOKEN_FAILURE\\n"); process.exitCode = 7;'], log, process.env, false);
    assert.equal(result.status, 7);
    const output = readFileSync(log, 'utf8');
    assert.match(output, /TOKEN_PASS/);
    assert.match(output, /TOKEN_FAILURE/);
  } finally {
    rmSync(directory, { recursive: true });
  }
});
