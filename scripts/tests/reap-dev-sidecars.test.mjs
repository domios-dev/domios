import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { once } from 'node:events';
import { promisify } from 'node:util';

const exec = promisify(execFile);

test('reaper kills the selected dev group and preserves foreign and prod fixtures', { timeout: 10000 }, async () => {
  const fixture = mkdtempSync(path.join(tmpdir(), 'sidecar ownership-'));
  const binary = path.join(fixture, 'codex');
  writeFileSync(binary, "require('node:child_process').spawn(process.execPath, ['-e', \"process.stdout.write('ready'); process.stdin.resume();\"], {stdio: ['pipe', 1, 2]}); process.stdin.resume();");
  const children = [];
  try {
    for (const instance of ['.codeterm-dev-own', '.codeterm-dev-other', '.codeterm']) {
      const token = path.join(fixture, instance, 'codex-rpc', 'a.token');
      const child = spawn(process.execPath, [binary, 'app-server', '--ws-token-file', token], { detached: true, stdio: ['pipe', 'pipe', 'pipe'] });
      children.push(child);
      await once(child.stdout, 'data');
    }
    const count = async () => (await exec(process.execPath, ['scripts/reap-dev-sidecars.mjs', path.join(fixture, '.codeterm-dev-own'), '--count'])).stdout.trim();
    assert.equal(await count(), '1');
    const exited = once(children[0], 'exit');
    const groupOutputClosed = once(children[0].stdout, 'end');
    await exec(process.execPath, ['scripts/reap-dev-sidecars.mjs', path.join(fixture, '.codeterm-dev-own')]);
    await exited;
    await groupOutputClosed;
    assert.equal(await count(), '0');
    assert.notEqual(children[0].exitCode, 0);
    for (const child of children.slice(1)) {
      assert.equal(child.exitCode, null);
      assert.equal(child.signalCode, null);
      assert.doesNotThrow(() => process.kill(child.pid, 0));
    }
  } finally {
    await Promise.all(children.map(async child => {
      if (child.exitCode !== null || child.signalCode !== null) return;
      const exited = once(child, 'exit');
      if (process.platform === 'win32') await exec('taskkill.exe', ['/PID', String(child.pid), '/T', '/F']);
      else process.kill(-child.pid, 'SIGKILL');
      await exited;
    }));
    rmSync(fixture, { recursive: true, force: true });
  }
});
