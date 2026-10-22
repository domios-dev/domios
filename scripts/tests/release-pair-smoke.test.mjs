import { strict as assert } from 'node:assert';
import test from 'node:test';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { earlyExitMessage, executionPlan, validateApiToken, recordSmokeReceipt, refusalCapture, reportServerLogTail, resolveExecution, scratchResidueMessage, targetCommand, validateReady } from '../release-pair-smoke.mjs';

test('a busy scratch directory after a clean exit names the leftover writer', () => {
  const error = Object.assign(new Error('ENOTEMPTY: directory not empty'), { code: 'ENOTEMPTY', path: '/tmp/smoke/isolated-data/channels/x/plugins' });
  assert.equal(scratchResidueMessage(error),
    'isolated server left a writer behind in its scratch directory at /tmp/smoke/isolated-data/channels/x/plugins: ENOTEMPTY');
  assert.match(scratchResidueMessage(new Error('boom')), /scratch directory: boom$/);
});

test('a failed smoke reports the isolated server log tail, or that none was written', async () => {
  const data = await mkdtemp(path.join(os.tmpdir(), 'pair-smoke-log-'));
  try {
    const written = [];
    await reportServerLogTail(data, { write: text => written.push(text) });
    assert.match(written[0], /wrote no log/);
    await mkdir(path.join(data, 'logs'));
    await writeFile(path.join(data, 'logs', 'domios.log'), ['line-a', 'line-b', 'line-c', ''].join('\n'));
    await reportServerLogTail(data, { lines: 2, write: text => written.push(text) });
    assert.equal(written[1], 'isolated server log tail:\nline-b\nline-c\n');
  } finally {
    await rm(data, { recursive: true, force: true });
  }
});

test('refusal diagnostics accept only three bounded product lines across stream chunks', () => {
  const capture = refusalCapture();
  capture.write(Buffer.from('unrelated secret output\n domios-server refused: not a prefix\ndomios-server ref'));
  capture.write(Buffer.from('used: first\r\ndomios-server refused: ' + 'x'.repeat(10000)));
  capture.write(Buffer.from('\ndomios-server refused: third\ndomios-server refused: fourth\n'));
  capture.finish();
  assert.deepEqual(capture.lines(), ['domios-server refused: first',
    ('domios-server refused: ' + 'x'.repeat(10000)).slice(0, 300), 'domios-server refused: third']);
});

test('refusal diagnostics preserve an unterminated product line but discard other stderr', () => {
  const capture = refusalCapture();
  capture.write(Buffer.from('token=never-forward-this\ndomios-server refused: launch failed'));
  capture.finish();
  assert.deepEqual(capture.lines(), ['domios-server refused: launch failed']);
});

test('early server exit distinguishes refusal from OS termination without runtime output', () => {
  assert.equal(earlyExitMessage(1, null), 'isolated server exited before READY (code=1, signal=none)');
  assert.equal(earlyExitMessage(null, 'SIGKILL'), 'isolated server exited before READY (code=none, signal=SIGKILL)');
});

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

test('isolated smoke accepts only a nonempty single-line credential from the local resolver', () => {
  assert.equal(validateApiToken('secret-token'), 'secret-token');
  for (const invalid of ['', ' ', 'one\ntwo', 'one\rtwo', null]) assert.throws(() => validateApiToken(invalid));
});
