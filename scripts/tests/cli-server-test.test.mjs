import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { prepareCliServer, requiresCliServer } from '../cli-server-test.mjs';

test('only a selected server integration prepares native artifacts', () => {
  const suite = (name, status) => ({ 'binary-name': name, testcases: { opaque: { 'filter-match': { status } } } });
  assert.equal(requiresCliServer({ 'rust-suites': { unit: suite('codeterm', 'matches') } }), false);
  assert.equal(requiresCliServer({ 'rust-suites': { native: suite('isolated_server', 'mismatch') } }), false);
  assert.equal(requiresCliServer({ 'rust-suites': { native: suite('isolated_server', 'matches') } }), true);
});

test('CLI integration preparation binds the exact CLI bytes into the debug server and stages both', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'cli-server-preparation-'));
  try {
    const cli = path.join(root, 'cli-artifact');
    const server = path.join(root, 'server-artifact');
    writeFileSync(cli, 'opaque-cli-bytes');
    writeFileSync(server, 'opaque-server-bytes');
    const calls = [];
    const cliTarget = path.join(root, 'cli-target');
    const serverTarget = path.join(root, 'server-target');
    const executable = prepareCliServer({ cliTarget, serverTarget, env: { FIXTURE: 'kept' },
      execute(command, args, options) {
        calls.push({ command, args, env: options.env });
        const binary = args[args.indexOf('--bin') + 1];
        const artifact = binary === 'codeterm' ? cli : server;
        return [JSON.stringify({ reason: 'compiler-artifact', target: { name: binary }, executable: artifact }),
          JSON.stringify({ reason: 'build-finished', success: true }), ''].join('\n');
      },
    });
    assert.equal(calls.length, 2);
    assert.equal(calls[0].env.CARGO_TARGET_DIR, cliTarget);
    assert.equal(calls[1].env.CARGO_TARGET_DIR, serverTarget);
    assert.equal(calls[1].env.CODETERM_SERVER_CLI_SHA256,
      createHash('sha256').update(readFileSync(cli)).digest('hex'));
    assert.ok(calls[1].args.includes('--no-default-features'));
    assert.equal(calls[1].args[calls[1].args.indexOf('-p') + 1], 'domios-server');
    assert.ok(calls.every(call => call.command === 'cargo' && call.args.includes('--locked') && call.env.FIXTURE === 'kept'));
    assert.equal(readFileSync(executable, 'utf8'), 'opaque-server-bytes');
    assert.equal(readFileSync(path.join(path.dirname(path.dirname(executable)), 'cli', 'codeterm' +
      (process.platform === 'win32' ? '.exe' : '')), 'utf8'), 'opaque-cli-bytes');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('CLI integration preparation refuses missing targets, failed builds and missing executable receipts', () => {
  assert.throws(() => prepareCliServer(), /both target/);
  const options = { cliTarget: 'cli-target', serverTarget: 'server-target' };
  const failure = new Error('opaque compiler failure');
  assert.throws(() => prepareCliServer({ ...options, execute() { throw failure; } }), error => error === failure);
  assert.throws(() => prepareCliServer({ ...options, execute() {
    return JSON.stringify({ reason: 'build-finished', success: true });
  } }), /Missing CLI integration artifact/);
});
