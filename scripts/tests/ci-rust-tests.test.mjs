import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import {
  MODES, candidateFilter, captureCommand, executedCount, executionGroups, generatedDirectories, platformFilter,
  requireTestSuccess, runArgs, timingAuditArgs, timingPlan, verifyReceipts,
} from '../ci-rust-tests.mjs';

const receipt = (overrides = {}) => ({ mode: 'full', runner: 'eight', os: 'Windows', arch: 'X64', sha: 'abc', executed: 6709, exit: 0, seconds: 128, ...overrides });
const complete = () => [receipt(), ...['macOS', 'Linux', 'Windows'].map((os) => receipt({ mode: 'platform', os, runner: os, executed: 40 }))];

test('one full suite and one platform subset per OS, each passing with executed tests', () => {
  assert.match(verifyReceipts(complete()), /\| full \| eight \| Windows \| X64 \| 6709 \| 128\.0 s \|/);
  for (const [name, mutate, message] of [
    ['missing full', (r) => r.shift(), /exactly one full/],
    ['duplicate full', (r) => r.push(receipt()), /exactly one full/],
    ['missing OS', (r) => r.pop(), /one Windows platform/],
    ['failed', (r) => { r[1].exit = 1; }, /Failed/],
    ['empty', (r) => { r[0].executed = 0; }, /No executed/],
    ['mixed source', (r) => { r[2].sha = 'def'; }, /Mixed/],
  ]) {
    const receipts = complete();
    mutate(receipts);
    assert.throws(() => verifyReceipts(receipts), message, name);
  }
});

test('platform selection keeps only cfg-gated, non-ignored tests per Cargo binary', () => {
  const row = (name, platform, ignored = false) => ({ name, platform, ignored, package: 'crate', binary: 'crate' });
  assert.equal(platformFilter([row('tests::a', true), row('tests::b', false), row('tests::c', true, true)]), '(package(=crate) & binary(=crate) & test(/^(?:tests::a)$/))');
  assert.equal(platformFilter([row('tests::b', false)]), 'none()');

  assert.throws(() => platformFilter([{ ...row('x', true), package: 'bad) | all(' }]), /Unsupported/);
});

test('the full suite runs the catalogue release set and leaves the native input crates to each OS', () => {
  assert.deepEqual(MODES.full.map((lane) => lane.lane), ['workspace', 'cli']);
  assert.deepEqual(MODES.platform.map((lane) => lane.lane), ['workspace', 'cli', 'input']);
  const workspace = MODES.full[0].targets;
  for (const pkg of ['codeterm', 'codeterm-mem-store', 'codeterm-paths', 'codeterm-sentinel']) assert.ok(workspace.includes(pkg), pkg);
  assert.ok(!workspace.includes('codeterm-input-macos') && !workspace.includes('portable-pty'));
  assert.deepEqual(workspace.slice(-3), ['--lib', '--test', 'main']);
  assert.equal(MODES.platform[2].whole, true);
});

test('build-script output directories come from cargo JSON messages', () => {
  const messages = ['{"reason":"build-script-executed","package_id":"p","out_dir":"/o"}', 'not json', '{"reason":"compiler-message"}'].join(String.fromCharCode(10));
  assert.deepEqual(generatedDirectories(messages), { p: '/o' });
});

test('executed counts sum every nextest summary', () => {
  assert.equal(executedCount('Summary [ 1.0s] 10 tests run: 10 passed\n\x1b[32mSummary\x1b[0m [ 2.0s] 1 test run: 1 passed\n'), 11);
  assert.equal(executedCount('no summary'), 0);
});

test('local runs compile in place with the lane targets and serialize only candidates', () => {
  const [workspace] = MODES.full;
  const primary = runArgs(workspace, 'CONFIG_TOKEN');
  const alone = runArgs(workspace, 'CONFIG_TOKEN', true);
  for (const args of [primary, alone]) {
    assert.equal(args[args.indexOf('--manifest-path') + 1], 'src-tauri/Cargo.toml');
    assert.equal(args[args.indexOf('--config-file') + 1], 'CONFIG_TOKEN');
    assert.ok(args.includes('--no-tests=fail'));
    assert.ok(!args.includes('--archive-file') && !args.includes('--partition'));
  }
  assert.ok(!primary.includes('--test-threads'));
  assert.equal(alone[alone.indexOf('--test-threads') + 1], '1');
});

test('only non-release macOS runs enforce with serial confirmation', () => {
  assert.deepEqual(timingPlan({ release: false, os: 'macOS' }), { confirm: true, policy: 'enforce' });
  for (const plan of [{ release: true, os: 'macOS' }, { release: false, os: 'Windows' }, { release: false, os: 'Linux' }]) {
    assert.deepEqual(timingPlan(plan), { confirm: false, policy: 'advisory' });
  }
  assert.deepEqual(timingAuditArgs(['GUARD'], 'ALONE', 'enforce'), ['GUARD', '--alone', 'ALONE', '--policy', 'enforce']);
  assert.deepEqual(timingAuditArgs(['GUARD'], undefined, 'advisory'), ['GUARD', '--policy', 'advisory']);
});

test('advisory audits pass on parallel timings while enforce requires serial confirmation', () => {
  const directory = mkdtempSync(join(tmpdir(), 'release-timing-'));
  try {
    const log = join(directory, 'parallel.log');
    const summary = join(directory, 'summary.md');
    writeFileSync(log, 'PASS [ 5.000s] codeterm_lib advisory_fixture_token\nSummary [ 5.000s] 1 tests run: 1 passed\n');
    const guard = ['scripts/check-slow-unit-tests.mjs', log, '--base', 'HEAD'];
    const run = (policy) => spawnSync(process.execPath, timingAuditArgs(guard, undefined, policy), { encoding: 'utf8', env: { ...process.env, GITHUB_STEP_SUMMARY: summary } });
    const advisory = run('advisory');
    assert.equal(advisory.status, 0, advisory.stderr);
    assert.match(advisory.stdout, /::warning::Unit timing \(parallel only\): 5\.000s.*advisory_fixture_token/);
    assert.match(readFileSync(summary, 'utf8'), /advisory_fixture_token/);
    assert.equal(run('enforce').status, 1);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('listener isolation preserves every selected test and omits only empty groups', () => {
  const normal = { name: 'tests::normal' };
  const listener = { name: 'startup::rest_api::tests::rebind' };
  assert.deepEqual(executionGroups([]), []);
  assert.deepEqual(executionGroups([normal]), [{ listener: false, selected: [normal] }]);
  assert.deepEqual(executionGroups([listener]), [{ listener: true, selected: [listener] }]);
  assert.deepEqual(executionGroups([normal, listener]).flatMap((g) => g.selected), [normal, listener]);
});

test('captured nextest logs settle both streams and retain failing process status', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'ci-tests-capture-'));
  try {
    const log = join(directory, 'nextest.log');
    const result = await captureCommand(process.execPath, ['-e', 'process.stdout.write("TOKEN_PASS\\n"); process.stderr.write("TOKEN_FAILURE\\n"); process.exitCode = 7;'], log, process.env, false);
    assert.equal(result.status, 7);
    assert.match(readFileSync(log, 'utf8'), /TOKEN_PASS[\s\S]*TOKEN_FAILURE|TOKEN_FAILURE[\s\S]*TOKEN_PASS/);
  } finally {
    rmSync(directory, { recursive: true });
  }
});

test('timing candidates are exact identities inside their original subgroup', () => {
  const selected = [{ binary: 'crate::bin/tool', name: 'tests::case' }];
  const document = { entries: [{ binary: 'crate::bin/tool', test: 'tests::case' }], filter: 'untrusted text' };
  assert.equal(candidateFilter(document, selected), '(binary_id(=crate::bin/tool) & test(=tests::case))');
  assert.equal(candidateFilter({ entries: [] }, selected), '');
  assert.throws(() => candidateFilter({}, selected), /Missing/);
  assert.throws(() => candidateFilter({ entries: [...document.entries, ...document.entries] }, selected), /Duplicate/);
  assert.throws(() => candidateFilter({ entries: [{ binary: 'other', test: 'tests::case' }] }, selected), /outside/);
  assert.throws(() => candidateFilter({ entries: [{ binary: 'crate) | all(', test: 'tests::case' }] }, selected), /Unsafe/);
});

test('failed, timed-out and killed executions are fatal; leaky passes warn', () => {
  requireTestSuccess({ status: 0 }, 'PASS [ 5.0s] crate slow\nSummary [ 5.0s] 1 tests run: 1 passed', 'primary');
  for (const status of ['FAIL', 'TIMEOUT']) assert.throws(() => requireTestSuccess({ status: 0 }, `${status} [ 5.0s] crate case`, 'primary'), /failed or timed-out/);
  for (const result of [{ status: 100 }, { status: 124 }, { status: null, signal: 'SIGTERM' }, { status: null, error: new Error('spawn failed') }]) {
    assert.throws(() => requireTestSuccess(result, 'PASS [ 0.1s] crate case', 'alone'));
  }
  const warnings = [];
  requireTestSuccess({ status: 0 }, 'LEAK [ 1.4s] codeterm_mem dreaming::tests::fixture', 'primary', { warn: (value) => warnings.push(value), summary: '' });
  assert.equal(warnings.length, 1);
  assert.ok(warnings[0].startsWith('::warning::'));
});

function inventory(ignored = false, status = 'listed') {
  return { 'rust-suites': { crate: { 'binary-id': 'crate', 'binary-name': 'crate', 'package-name': 'crate', status, testcases: { 'tests::a': { ignored } } } } };
}
test('classification joins compiled names and inherited platform gates', async () => {
  const { classifyInventory } = await import('../ci-rust-classification.mjs');
  assert.equal(classifyInventory(inventory(), { crate: [{ name: 'tests::a', platform: true }] })[0].platform, true);
  const parameterized = inventory();
  parameterized['rust-suites'].crate.testcases = { 'tests::a::case_1': { ignored: false } };
  assert.equal(classifyInventory(parameterized, { crate: [{ name: 'tests::a', platform: true }] })[0].platform, true);
  assert.throws(() => classifyInventory(inventory(), { crate: [] }), /Unclassified/);
  assert.throws(() => classifyInventory(inventory(true), { crate: [{ name: 'tests::a' }] }), /Ignored/);
  assert.throws(() => classifyInventory(inventory(false, 'skipped'), {}), /Unlisted/);
  const explained = classifyInventory(inventory(true), { crate: [{ name: 'tests::a', platform: true, ignore_reason: 'TOKEN_REASON' }] });
  assert.equal(explained[0].reason, 'TOKEN_REASON');
  assert.equal(platformFilter(explained), 'none()', 'an ignored test never counts as executed platform coverage');
});
