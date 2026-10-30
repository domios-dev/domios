import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { once } from 'node:events';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { acquireLaunchOwner, retainLaunchOwner } from '../dev-launch-owner.mjs';
import { acquireLaunchFileLock } from '../dev-launch-file-lock.mjs';

test('concurrent target launch refuses once with a typed busy diagnostic before running its child', { timeout: 15000 }, async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'domios-target-busy-'));
  const script = fileURLToPath(new URL('../dev-launch-owner.mjs', import.meta.url));
  const holder = await acquireLaunchFileLock(root);
  try {
    await assert.rejects(acquireLaunchFileLock(root), { code: 'DEV_LAUNCH_BUSY', exitCode: 75 });
    for (const args of [['--target-reservation', root], [root]]) {
      const result = spawnSync(process.execPath, [script, ...args, process.execPath, '-e', 'console.log("UNSAFE_ADMITTED"); process.exit(99)'],
        { encoding: 'utf8', windowsHide: true, timeout: 10_000,
          env: Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('CODETERM_DEV_LAUNCH_') && !key.startsWith('CODETERM_DEV_TARGET_'))) });
      assert.equal(result.status, 75, result.stderr || result.error?.message);
      assert.equal(result.stdout, '');
      assert.match(result.stderr, args.length === 2 ? /^DEV_TARGET_BUSY:.*native READY or exit/s : /^DEV_LAUNCH_BUSY:/);
      assert.doesNotMatch(result.stderr, /CategoryInfo|FullyQualifiedErrorId|MethodInvocationException|Win32Exception|UNSAFE_ADMITTED/);
    }
  } finally {
    await holder.release();
    rmSync(root, { recursive: true, force: true });
  }
});

test('one launcher owns a tree until its supervisor lease closes; other instances remain independent', { timeout: 30000 }, async () => {
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

test('abrupt file-lock holder death releases the OS lock despite its surviving file', { timeout: 15000 }, async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'domios-launch-crash-'));
  const holder = await acquireLaunchFileLock(root);
  let replacement;
  try {
    await assert.rejects(acquireLaunchFileLock(root), /DEV launch owner unavailable/);
    const exited = once(holder.child, 'close');
    holder.child.kill('SIGKILL');
    await exited;
    replacement = await acquireLaunchFileLock(root);
  } finally {
    await holder.release();
    if (replacement) await replacement.release();
    rmSync(root, { recursive: true, force: true });
  }
});
