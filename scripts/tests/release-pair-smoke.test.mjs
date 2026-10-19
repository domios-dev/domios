import { strict as assert } from 'node:assert';
import test from 'node:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { executionPlan, parseApiToken, recordSmokeReceipt, resolveExecution, targetCommand, validateReady } from '../release-pair-smoke.mjs';

test('Windows ARM64 on x64 refuses any native spawn and records the architecture gap', async () => {
  const plan = await resolveExecution(executionPlan('aarch64-pc-windows-msvc', 'win32', 'x64'), () => {
    assert.fail('Windows must not probe Rosetta');
  });
  assert.equal(plan.mode, 'unverified_cross_arch');
  assert.equal(plan.host_arch, 'x64');
  assert.equal(plan.target_arch, 'arm64');
  assert.throws(() => targetCommand('server.exe', [], plan), /must not spawn/);
});

test('macOS x64 on Apple Silicon executes server and CLI through available Rosetta', async () => {
  const plan = await resolveExecution(executionPlan('x86_64-apple-darwin', 'darwin', 'arm64'), async () => true);
  assert.equal(plan.mode, 'rosetta');
  assert.deepEqual(targetCommand('/tmp/server', ['start'], plan),
    { binary: '/usr/bin/arch', args: ['-x86_64', '/tmp/server', 'start'] });
  assert.deepEqual(targetCommand('/tmp/cli', ['identify', '--json'], plan),
    { binary: '/usr/bin/arch', args: ['-x86_64', '/tmp/cli', 'identify', '--json'] });
});

test('unavailable Rosetta yields an explicit unverified result while native hosts execute directly', async () => {
  const plan = await resolveExecution(executionPlan('x86_64-apple-darwin', 'darwin', 'arm64'), async () => false);
  assert.equal(plan.mode, 'unverified_cross_arch');
  assert.equal(plan.reason, 'rosetta_unavailable_or_failed_to_start');
  const native = await resolveExecution(executionPlan('x86_64-unknown-linux-gnu', 'linux', 'x64'));
  assert.deepEqual(targetCommand('/tmp/server', ['start'], native), { binary: '/tmp/server', args: ['start'] });
});

test('cross-architecture receipt persists byte verification and surfaces UNVERIFIED without PASS', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'codeterm-smoke-receipt-'));
  try {
    const receipt = { target: 'aarch64-pc-windows-msvc', status: 'unverified_cross_arch', byte_checks: 'passed',
      execution: executionPlan('aarch64-pc-windows-msvc', 'win32', 'x64'),
      signing: 'updater-signature only; no Authenticode' };
    const summary = path.join(directory, 'summary.md');
    await recordSmokeReceipt(receipt, directory, summary);
    assert.deepEqual(JSON.parse(await readFile(path.join(directory, `release-pair-smoke-${receipt.target}.json`), 'utf8')), receipt);
    const report = await readFile(summary, 'utf8');
    assert.match(report, /UNVERIFIED/);
    assert.doesNotMatch(report, /\bPASS\b/);
    assert.match(report, /updater-signature only; no Authenticode/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test('isolated smoke accepts only exact server READY identity', () => {
  assert.deepEqual(validateReady('{"type":"READY","build":"abc","port":49152}', 'abc', 49152),
    { type: 'READY', build: 'abc', port: 49152 });
  assert.throws(() => validateReady('{"type":"READY","build":"other","port":49152}', 'abc', 49152));
  assert.throws(() => validateReady('{"type":"READY","build":"abc","port":49152,"token":"secret"}', 'abc', 49152));
});

test('isolated smoke reads the API token only from the api TOML section', () => {
  assert.equal(parseApiToken('[other]\ntoken = "wrong"\n[api]\ntoken = "secret-token"\n'), 'secret-token');
  assert.throws(() => parseApiToken('[other]\ntoken = "wrong"\n'));
  assert.throws(() => parseApiToken('[api]\ntoken = "one"\ntoken = "two"\n'));
});
