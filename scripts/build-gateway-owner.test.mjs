// Caller: build.sh script-guards; skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import readline from 'node:readline';
import test from 'node:test';
import { ownerState } from './build-gateway-owner.mjs';
import { nativeIdentity } from './build-gateway-guard.mjs';

const root = path.resolve(import.meta.dirname, '..');
const windowsOwner = { pid: '501', start_token: 'msys-start', native_pid: '9001', native_start_token: 'windows-start' };

test('Windows reaping uses the captured native waiter identity rather than its MSYS number', () => {
  assert.equal(ownerState(windowsOwner, pid => { assert.equal(pid, '9001'); return { state: 'present', token: 'windows-start' }; }, 'win32'), 'alive');
  assert.equal(ownerState(windowsOwner, () => ({ state: 'gone' }), 'win32'), 'gone');
  assert.equal(ownerState(windowsOwner, () => ({ state: 'present', token: 'replacement-start' }), 'win32'), 'gone');
});

test('unreadable or incomplete native evidence is unknown and never a reap decision', () => {
  assert.equal(ownerState(windowsOwner, () => ({ state: 'unknown' }), 'win32'), 'unknown');
  assert.equal(ownerState(windowsOwner, () => ({ state: 'present' }), 'win32'), 'unknown');
  assert.equal(ownerState({ pid: '501', start_token: 'msys-start' }, () => { throw new Error('wrong namespace must not be probed'); }, 'win32'), 'unknown');
});

test('POSIX owners retain their own namespace and start token', () => {
  for (const platform of ['linux', 'darwin']) {
    assert.equal(ownerState(windowsOwner, pid => { assert.equal(pid, '501'); return { state: 'present', token: 'msys-start' }; }, platform), 'alive');
  }
});

function observed(directory, predicate) {
  return new Promise((resolve, reject) => {
    let timer;
    const watcher = fs.watch(directory, { recursive: true }, check);
    function finish(error, value) { clearTimeout(timer); watcher.close(); error ? reject(error) : resolve(value); }
    function check() { try { const value = predicate(); if (value) finish(null, value); } catch (error) { if (!['ENOENT', 'EACCES', 'EPERM'].includes(error.code)) finish(error); } }
    timer = setTimeout(() => finish(new Error('Waiter ownership event deadline exceeded')), 20000);
    check();
  });
}
const quote = text => "'" + text.replaceAll("'", "'\\''") + "'";

test('a PowerShell build.ps1 queued waiter survives a native-namespace reap pass while alive', { skip: process.platform !== 'win32', timeout: 60000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-powershell-waiter-'));
  const state = path.join(directory, 'state');
  fs.mkdirSync(path.join(state, 'jobs'), { recursive: true });
  fs.mkdirSync(path.join(state, 'queue'));
  const bash = 'C:/Program Files/Git/bin/bash.exe';
  const source = `source ${quote(path.join(root, 'scripts/build-gateway.sh').replaceAll('\\', '/'))}\nct_gateway_set_fixture_dir ${quote(state.replaceAll('\\', '/'))}\n`;
  const children = [];
  function launch(command, args, extra = {}) {
    const child = spawn(command, args, { cwd: directory, env: { ...process.env, ...extra }, stdio: ['pipe', 'pipe', 'pipe'] });
    const lines = readline.createInterface({ input: child.stdout });
    let output = '', errors = '';
    child.stdout.on('data', chunk => { output += chunk; });
    child.stderr.on('data', chunk => { errors += chunk; });
    const exited = once(child, 'close').then(([code]) => ({ code, output, errors }));
    const result = { child, lines, exited };
    children.push(result);
    return result;
  }
  const holder = launch(bash, ['-c', source + `
    CT_BUILD_JOB_ID=holder
    CODETERM_BUILD_GATEWAY_MODE=queue
    ct_gateway_acquire 'holder fixture' || exit $?
    echo HOLDER_READY
    read -r release
    ct_gateway_finish 0
  `]);
  try {
    assert.equal((await once(holder.lines, 'line'))[0], 'HOLDER_READY');
    fs.writeFileSync(path.join(directory, 'build.ps1'), fs.readFileSync(path.join(root, 'build.ps1'), 'utf8'));
    fs.writeFileSync(path.join(directory, 'build.sh'), source + `
      waiter_pid="$BASHPID"
      waiter_windows_pid="$(ct_gateway_native_pid "$waiter_pid")"
      printf 'WAITER %s %s\\n' "$waiter_pid" "$waiter_windows_pid"
      CT_BUILD_JOB_ID=waiter
      CODETERM_BUILD_GATEWAY_MODE=queue
      CODETERM_BUILD_GATEWAY_WAIT_SECS=30
      ct_gateway_acquire 'check-queue fixture' || exit $?
      echo COMMAND_REACHED
      ct_gateway_finish 0
    `);
    const waiter = launch('powershell.exe', ['-NoProfile', '-File', path.join(directory, 'build.ps1'), 'check-queue']);
    const identityLine = (await once(waiter.lines, 'line'))[0];
    const [, msysPid, windowsPid] = identityLine.match(/^WAITER (\d+) (\d+)$/);
    const record = await observed(state, () => {
      const file = path.join(state, 'jobs/waiter.json');
      if (!fs.existsSync(file)) return false;
      const value = JSON.parse(fs.readFileSync(file, 'utf8'));
      return value.state === 'queued' && value;
    });
    assert.equal(String(record.owner_pid), msysPid);
    assert.equal(String(record.owner_native_pid), windowsPid);
    assert.equal(record.owner_pid_namespace, 'msys');
    assert.equal(nativeIdentity(windowsPid).token, record.owner_native_start_token);
    const reap = launch(bash, ['-c', source + `
      ct_gateway_mutex_acquire ${quote(state.replaceAll('\\', '/'))} || exit $?
      ct_gateway_pid_namespace() { echo native; }
      ct_gateway_reap_stale_queue_locked ${quote(state.replaceAll('\\', '/'))} || exit $?
      ct_gateway_mutex_release ${quote(state.replaceAll('\\', '/'))}
    `]);
    const reaped = await reap.exited;
    assert.equal(reaped.code, 0, reaped.errors);
    assert.doesNotMatch(reaped.errors, /vanished|owner_lost/);
    assert.equal(fs.readFileSync(path.join(state, 'jobs/waiter.json'), 'utf8').includes('"state": "queued"'), true);
    assert.ok(fs.existsSync(path.join(state, 'queue', record.queue_key)));
    holder.child.stdin.end('release\n');
    assert.equal((await holder.exited).code, 0);
    const finished = await waiter.exited;
    assert.equal(finished.code, 0, finished.errors);
    assert.match(finished.output, /COMMAND_REACHED/);
    assert.equal(JSON.parse(fs.readFileSync(path.join(state, 'jobs/waiter.json'), 'utf8')).state, 'completed');
  } finally {
    for (const result of children) {
      result.lines.close();
      if (result.child.exitCode === null) result.child.kill('SIGKILL');
    }
    await Promise.all(children.map(result => result.exited));
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
