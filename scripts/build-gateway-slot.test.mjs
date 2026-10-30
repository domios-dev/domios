// Caller: skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import readline from 'node:readline';
import test from 'node:test';
import { slotSnapshot, waitForSlotChange } from './build-gateway-slot.mjs';

const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';

function gatewayDiagnosticTrace(directory, stage) {
  const events = [], watchers = new Map();
  let dropped = 0, stopped = false;
  const read = file => {
    let fd;
    try {
      fd = fs.openSync(file, 'r');
      const buffer = Buffer.alloc(1024);
      const size = fs.readSync(fd, buffer, 0, buffer.length, 0);
      return buffer.subarray(0, size).toString('utf8').trim();
    } catch (error) { return { unavailable: error.code }; }
    finally { if (fd !== undefined) fs.closeSync(fd); }
  };
  const tuple = location => Object.fromEntries(['job_id', 'pid', 'start_token', 'pid_namespace', 'native_pid', 'native_start_token']
    .map(key => [key, read(path.join(location, key))]));
  const names = location => {
    try { const all = fs.readdirSync(location).sort(); return { total: all.length, entries: all.slice(0, 16) }; }
    catch (error) { return { unavailable: error.code, entries: [] }; }
  };
  const mark = (label, force = false) => {
    if (stopped) return;
    if (events.length === 64) { dropped++; if (!force) return; events.shift(); }
    events.push({ at: Date.now(), stage: stage(), label: String(label).slice(0, 160),
      slot: tuple(path.join(directory, 'lock')), metadata: tuple(path.join(directory, 'state.lock')),
      metadataPublication: read(path.join(directory, 'state.lock/metadata.json')),
      publicationGuard: read(path.join(directory, 'state.lock.guard')), queue: names(path.join(directory, 'queue')) });
  };
  const attach = () => {
    if (stopped) return;
    for (const location of [...(path.basename(directory) === 'state' ? [path.dirname(directory)] : []), directory, path.join(directory, 'queue'), path.join(directory, 'jobs'), path.join(directory, 'lock'), path.join(directory, 'state.lock')]) {
      if (watchers.has(location) || !fs.existsSync(location)) continue;
      try {
        const watcher = fs.watch(location, (event, file) => { if (events.length === 64) { dropped++; return; } mark(`${path.basename(location)}:${event}:${file ?? ''}`); attach(); });
        watcher.on('error', error => mark(`watch-error:${error.code}`));
        watchers.set(location, watcher);
      } catch (error) { mark(`watch-unavailable:${error.code}`); }
    }
  };
  const snapshot = () => {
    const jobs = names(path.join(directory, 'jobs'));
    return { directory, dropped, events, current: { slot: tuple(path.join(directory, 'lock')), metadata: tuple(path.join(directory, 'state.lock')),
      publicationGuard: read(path.join(directory, 'state.lock.guard')), queue: names(path.join(directory, 'queue')) },
      jobs: jobs.entries.filter(name => name.endsWith('.json')).map(name => {
        try {
          const file = path.join(directory, 'jobs', name);
          const size = fs.statSync(file).size;
          if (size > 16384) return { name, bytes: size, truncated: true };
          const row = JSON.parse(fs.readFileSync(file, 'utf8'));
          return Object.fromEntries(['job_id', 'state', 'owner_pid', 'owner_start_token', 'owner_pid_namespace', 'owner_native_pid', 'owner_native_start_token', 'queue_key', 'exit_code', 'outcome'].map(key => [key, row[key]]));
        } catch (error) { return { name, unavailable: error.code ?? error.name }; }
      }), jobsTotal: jobs.total };
  };
  mark('trace-start'); attach();
  return { mark: label => { mark(label, true); attach(); }, sample: label => { if (events.length === 64) { dropped++; return; } mark(label); attach(); }, snapshot, close() { stopped = true; for (const watcher of watchers.values()) watcher.close(); } };
}

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
    const watcher = fs.watch(directory, check);
    watcher.on('error', finish);
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
  let errors = '', output = '';
  child.stdout.on('data', chunk => output += chunk);
  let markQueued;
  const queued = new Promise(resolve => markQueued = resolve);
  child.stderr.on('data', chunk => { errors += chunk; if (errors.includes('Build gateway queued:')) markQueued(); });
  const lines = readline.createInterface({ input: child.stdout });
  const ready = new Promise(resolve => lines.once('line', resolve));
  const running = new Promise(resolve => lines.on('line', line => { if (line === 'RUNNING') resolve(); }));
  const exited = once(child, 'exit').then(([code]) => ({ code, errors }));
  return { id, child, ready, running, queued, exited, get errors() { return errors; }, get output() { return output; } };
}

test('four parallel check-queue gateway clients finish in ticket order with metadata mutex free during slot wait', { timeout: 120000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-clients-'));
  fs.mkdirSync(path.join(directory, 'queue'));
  fs.mkdirSync(path.join(directory, 'jobs'));
  const observers = [];
  let phase = 'startup';
  const trace = gatewayDiagnosticTrace(directory, () => phase);
  let lastStatus;
  const status = async () => {
    const observer = spawn(bash, ['-c', 'source scripts/build-gateway.sh; ct_repo_root() { printf "%s\\n" "$PWD"; }; ct_gateway_set_fixture_dir "$1"; ct_gateway_status', 'fixture', directory], { cwd: root, env: { ...process.env, PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH }, stdio: ['ignore', 'pipe', 'pipe'] });
    observers.push(observer);
    let output = '', errors = '';
    observer.stdout.on('data', chunk => output += chunk);
    observer.stderr.on('data', chunk => errors += chunk);
    const [code] = await once(observer, 'exit');
    assert.equal(code, 0, errors);
    lastStatus = JSON.parse(output);
    trace.mark('status result');
    return lastStatus;
  };
  const clients = Array.from({ length: 4 }, (_, index) => client(directory, `client-${index}`));
  for (const current of clients) {
    current.child.stdout.on('data', () => trace.sample(`client-output:${current.id}`));
    current.child.stderr.on('data', () => trace.sample(`client-stderr:${current.id}`));
  }
  let deadline;
  const failed = new Promise((_, reject) => { deadline = setTimeout(() => reject(new Error(`Fixture event deadline at ${phase}`)), 100000); });
  try {
    await Promise.race([failed, (async () => {
    phase = 'client readiness';
    await Promise.all(clients.map(c => c.ready));
    trace.mark('all clients READY');
    for (const c of clients) c.child.stdin.write('start\n');
    trace.mark('start released to four clients');
    phase = 'first running receipt';
    const first = await Promise.race(clients.map(async c => { await c.running; return c; }));
    phase = 'three queued receipts';
    await Promise.all(clients.filter(c => c !== first).map(c => c.queued));
    phase = 'queue status';
    const records = await status();
    assert.equal(records.active.job_id, first.id);
    assert.equal(records.queue.length, 3);
    const order = [first.id, ...records.queue.map(j => j.job_id)];
    await observe(directory, () => !fs.existsSync(path.join(directory, 'state.lock')));
    assert.equal(fs.existsSync(path.join(directory, 'state.lock')), false);
    for (const id of order) {
      phase = `running receipt ${id}`;
      const current = clients.find(c => c.id === id);
      await current.running;
      phase = `running status ${id}`;
      assert.equal((await status()).active.job_id, id);
      phase = `completion ${id}`;
      trace.mark(`before release:${id}`);
      current.child.stdin.end('release\n');
      const result = await current.exited;
      trace.mark(`after exit:${id}`);
      assert.equal(result.code, 0, result.errors);
      assert.doesNotMatch(result.errors, /EPERM|Timed out|owner_lost/);
    }
    phase = 'final status';
    const final = await status();
    assert.equal(final.recent.length, 4);
    assert.ok(final.recent.every(j => j.state === 'completed' && j.exit_code === 0 && j.outcome === 'success'));
    assert.equal(fs.existsSync(path.join(directory, 'lock')), false);
    })()]);
  } catch (error) {
    trace.mark('failure snapshot');
    throw new Error(`${error.message}\n${JSON.stringify({ phase, lastStatus, trace: trace.snapshot(), clients: clients.map(c => ({ id: c.id, nativeChildPid: c.child.pid, exitCode: c.child.exitCode, signal: c.child.signalCode, output: c.output.slice(-4096), errors: c.errors.slice(-4096) })) })}`, { cause: error });
  } finally {
    trace.close();
    clearTimeout(deadline);
    for (const child of [...clients.map(c => c.child), ...observers]) if (child.exitCode === null && child.signalCode === null) {
      if (process.platform === 'win32') spawnSync('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
      else child.kill('SIGKILL');
    }
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

test('slot wait watches state directories without traversing artifact subtrees', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-slot-'));
  const artifacts = path.join(directory, 'targets');
  fs.mkdirSync(path.join(directory, 'queue'));
  fs.mkdirSync(artifacts);
  const watched = [];
  const watch = (location, callback) => {
    assert.equal(typeof callback, 'function', 'slot subscriptions must be non-recursive');
    assert.ok(!location.startsWith(artifacts));
    watched.push(location);
    return fs.watch(location, callback);
  };
  try {
    const changed = waitForSlotChange(directory, slotSnapshot(directory), Date.now() + 2000, { watch });
    fs.mkdirSync(path.join(directory, 'queue', 'ticket'));
    await changed;
    assert.ok(watched.includes(directory));
    assert.ok(watched.includes(path.join(directory, 'queue')));
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

for (const field of ['job_id', 'pid', 'start_token', 'pid_namespace', 'native_pid', 'native_start_token'])
test(`slot wait observes owner ${field} without a queue change`, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-slot-'));
  fs.mkdirSync(path.join(directory, 'queue'));
  fs.mkdirSync(path.join(directory, 'lock'));
  const owner = path.join(directory, 'lock', field);
  fs.writeFileSync(owner, 'before');
  try {
    const changed = waitForSlotChange(directory, slotSnapshot(directory), Date.now() + 2000);
    fs.writeFileSync(owner, 'after');
    await changed;
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
