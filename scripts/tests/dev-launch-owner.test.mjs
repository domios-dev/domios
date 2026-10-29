import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { acquireLaunchOwner, retainLaunchOwner } from '../dev-launch-owner.mjs';

test('one launcher owns a tree until its supervisor lease closes; other instances remain independent', { timeout: 10000 }, async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'domios-launch-owner-'));
  const foreign = mkdtempSync(path.join(tmpdir(), 'domios-foreign-owner-'));
  let owner, other, lease;
  try {
    owner = await acquireLaunchOwner(root);
    other = await acquireLaunchOwner(foreign);
    await assert.rejects(acquireLaunchOwner(root), /DEV launch owner unavailable/);
    lease = await retainLaunchOwner(owner.environment);
    const closed = owner.finish();
    await assert.rejects(acquireLaunchOwner(root), /DEV launch owner unavailable/);
    lease.end();
    await closed;
    owner = await acquireLaunchOwner(root);
  } finally {
    lease?.destroy();
    if (owner) await owner.finish();
    if (other) await other.finish();
    rmSync(root, { recursive: true, force: true });
    rmSync(foreign, { recursive: true, force: true });
  }
});

for (const harness of ['node', 'bash']) test(`launcher retains ownership through ${harness} command lifetime`, { timeout: 15000 }, async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'domios-launch-command-'));
  const script = fileURLToPath(new URL('../dev-launch-owner.mjs', import.meta.url));
  const environment = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('CODETERM_DEV_LAUNCH_')));
  const command = harness === 'node'
    ? [process.execPath, '-e', 'process.stdout.write("ready\\n"); process.stdin.once("data", () => process.exit(0));']
    : [process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash', '-c', 'printf "ready\\n"; read -r completion'];
  const child = spawn(process.execPath, [script, root, ...command],
  { env: environment, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  const exited = once(child, 'exit');
  let error = '';
  child.stderr.on('data', bytes => { error += bytes; });
  try {
    await once(child.stdout, 'data');
    await assert.rejects(acquireLaunchOwner(root), /DEV launch owner unavailable/);
    child.stdin.end('stop\n');
    assert.equal((await exited)[0], 0, error);
  } finally {
    if (child.exitCode === null) child.kill();
    rmSync(root, { recursive: true, force: true });
  }
});

test('abrupt lock-owner death leaves no stale filesystem lock', { timeout: 10000 }, async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'domios-launch-crash-'));
  const script = fileURLToPath(new URL('../dev-launch-owner.mjs', import.meta.url));
  const child = spawn(process.execPath, [script, '--serve-lock', root], {
    stdio: ['ignore', 'ignore', 'ignore', 'ipc'], windowsHide: true,
  });
  let lease, replacement;
  try {
    const [environment] = await once(child, 'message');
    assert.equal(environment.error, undefined);
    lease = await retainLaunchOwner(environment);
    child.send('leased');
    const disconnected = new Promise(resolve => lease.once('close', resolve));
    const exited = once(child, 'exit');
    child.kill('SIGKILL');
    await Promise.all([disconnected, exited]);
    replacement = await acquireLaunchOwner(root);
  } finally {
    lease?.destroy();
    if (child.exitCode === null && child.signalCode === null) child.kill();
    if (replacement) await replacement.finish();
    rmSync(root, { recursive: true, force: true });
  }
});
