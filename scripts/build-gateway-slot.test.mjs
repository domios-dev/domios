// Caller: skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import readline from 'node:readline';
import test from 'node:test';
import { slotSnapshot, waitForSlotChange } from './build-gateway-slot.mjs';

const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
function observe(directory, predicate) {
  return new Promise((resolve, reject) => {
    let timer;
    const finish = (error, value) => {
      clearTimeout(timer); watcher.close();
      if (error) reject(error); else resolve(value);
    };
    const check = () => {
      try { const value = predicate(); if (value) finish(null, value); }
      catch (error) { if (error.code !== 'ENOENT') finish(error); }
    };
    const watcher = fs.watch(directory, { recursive: true }, check);
    timer = setTimeout(() => finish(new Error('Fixture transition deadline exceeded')), 60000);
    check();
  });
}
function client(directory, id) {
  const child = spawn(bash, ['-c', `
    source scripts/build-gateway.sh
    ct_repo_root() { printf '%s\\n' "$PWD"; }
    ct_gateway_set_fixture_dir "$1"
    CODETERM_BUILD_GATEWAY_MODE=queue
    CODETERM_INSTANCE=fixture
    CT_BUILD_JOB_ID="$2"
    echo READY
    read -r start
    ct_gateway_acquire "check-queue fixture-$2" || exit $?
    echo RUNNING
    read -r release
    ct_gateway_finish 0
  `, 'fixture', directory, id], { cwd: root, env: { ...process.env, PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH }, stdio: ['pipe', 'pipe', 'pipe'] });
  let errors = '';
  let markQueued;
  const queued = new Promise(resolve => markQueued = resolve);
  child.stderr.on('data', chunk => { errors += chunk; if (errors.includes('Build gateway queued:')) markQueued(); });
  const lines = readline.createInterface({ input: child.stdout });
  const ready = new Promise(resolve => lines.once('line', resolve));
  const running = new Promise(resolve => lines.on('line', line => { if (line === 'RUNNING') resolve(); }));
  const exited = once(child, 'exit').then(([code]) => ({ code, errors }));
  return { id, child, ready, running, queued, exited, get errors() { return errors; } };
}

test('four parallel check-queue gateway clients finish in ticket order with metadata mutex free during slot wait', { timeout: 120000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-clients-'));
  fs.mkdirSync(path.join(directory, 'queue'));
  fs.mkdirSync(path.join(directory, 'jobs'));
  const status = async () => {
    const observer = spawn(bash, ['-c', 'source scripts/build-gateway.sh; ct_repo_root() { printf "%s\\n" "$PWD"; }; ct_gateway_set_fixture_dir "$1"; ct_gateway_status', 'fixture', directory], { cwd: root, env: { ...process.env, PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH }, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '', errors = '';
    observer.stdout.on('data', chunk => output += chunk);
    observer.stderr.on('data', chunk => errors += chunk);
    const [code] = await once(observer, 'exit');
    assert.equal(code, 0, errors);
    return JSON.parse(output);
  };
  const clients = Array.from({ length: 4 }, (_, index) => client(directory, `client-${index}`));
  try {
    await Promise.all(clients.map(c => c.ready));
    for (const c of clients) c.child.stdin.write('start\n');
    const first = await Promise.race(clients.map(async c => { await c.running; return c; }));
    await Promise.all(clients.filter(c => c !== first).map(c => c.queued));
    const records = await status();
    assert.equal(records.active.job_id, first.id);
    assert.equal(records.queue.length, 3);
    const order = [first.id, ...records.queue.map(j => j.job_id)];
    await observe(directory, () => !fs.existsSync(path.join(directory, 'state.lock')));
    assert.equal(fs.existsSync(path.join(directory, 'state.lock')), false);
    for (const id of order) {
      const current = clients.find(c => c.id === id);
      await current.running;
      assert.equal((await status()).active.job_id, id);
      current.child.stdin.end('release\n');
      const result = await current.exited;
      assert.equal(result.code, 0, result.errors);
      assert.doesNotMatch(result.errors, /EPERM|Timed out|owner_lost/);
    }
    assert.ok((await status()).recent.every(j => j.state === 'completed' && j.exit_code === 0 && j.outcome === 'success'));
    assert.equal(fs.existsSync(path.join(directory, 'lock')), false);
  } catch (error) {
    throw new Error(`${error.message}\n${JSON.stringify(await status())}\n${clients.map(c => `${c.id}: exit=${c.child.exitCode} ${c.errors}`).join('\n')}`);
  } finally {
    for (const c of clients) if (c.child.exitCode === null) c.child.kill('SIGKILL');
    await Promise.all(clients.map(c => c.exited));
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('slot change before watcher registration is observed', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-slot-'));
  fs.mkdirSync(path.join(directory, 'queue'));
  const before = slotSnapshot(directory);
  fs.mkdirSync(path.join(directory, 'queue', 'ticket'));
  try { await waitForSlotChange(directory, before, Date.now() + 2000); }
  finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('unchanged slot reaches a bounded failure deadline', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-slot-'));
  fs.mkdirSync(path.join(directory, 'queue'));
  try { await assert.rejects(waitForSlotChange(directory, slotSnapshot(directory), Date.now()), /Timed out waiting/); }
  finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
