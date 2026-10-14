// Caller: build.sh script-guards; skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import readline from 'node:readline';
import test from 'node:test';
const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const env = { ...process.env, PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH };
function observed(directory, predicate) {
  return new Promise((resolve, reject) => {
    let timer;
    const watcher = fs.watch(directory, { recursive: true }, check);
    function finish(error, value) { clearTimeout(timer); watcher.close(); error ? reject(error) : resolve(value); }
    function check() { try { const value = predicate(); if (value) finish(null, value); } catch (e) { if (e.code !== 'ENOENT') finish(e); } }
    timer = setTimeout(() => finish(new Error('Fixture event deadline exceeded')), 30000);
    check();
  });
}
function shell(directory, body, id = 'fixture') {
  const child = spawn(bash, ['-c', body, 'fixture', directory, id], { cwd: root, env, stdio: ['pipe', 'pipe', 'pipe'] });
  let output = '', errors = '';
  child.stdout.on('data', c => output += c);
  child.stderr.on('data', c => errors += c);
  const lines = readline.createInterface({ input: child.stdout });
  const ready = new Promise(resolve => lines.once('line', resolve));
  const exited = once(child, 'exit').then(([code]) => ({ code, output, errors }));
  return { child, ready, exited, id };
}
async function cleanup(directory, clients) {
  for (const c of clients) if (c.child.exitCode === null) {
    c.child.stdin.destroy();
    if (process.platform === 'win32') spawnSync('taskkill.exe', ['/PID', String(c.child.pid), '/T', '/F'], { stdio: 'ignore' });
    else c.child.kill('SIGKILL');
  }
  await Promise.all(clients.map(c => c.exited));
  fs.rmSync(directory, { recursive: true, force: true });
}
const statusBody = 'source scripts/build-gateway.sh; ct_gateway_set_fixture_dir "$1/state"; ct_gateway_status';

test('four concurrent cache clients start the server exactly once and leave gateway metadata free', { timeout: 60000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-sccache-'));
  const clients = Array.from({ length: 4 }, (_, i) => shell(directory, `
    source scripts/cache-policy.sh
    source scripts/build-gateway.sh
    ct_gateway_set_fixture_dir "$1/state"
    fixture_dir="$1"; fixture_id="$2"
    ct_verify_target_dir() { :; }
    ct_forbid_mutable_cache_dir() { :; }
    ct_reject_basedir_rewriting() { :; }
    ct_sccache_storage() { CT_SCCACHE_STORAGE=local; }
    ct_sccache_print_location() { :; }
    ct_sccache_server_live() { [ -f "$fixture_dir/live" ]; }
    pgrep() { return 1; }
    powershell.exe() { return 1; }
    sccache() {
      case "$1" in
        --version) echo fixture ;;
        --stop-server) echo stop >> "$fixture_dir/events" ;;
        --start-server)
          mkdir "$fixture_dir/start-claimed" || { echo 'os error 10048' >&2; return 1; }
          echo start >> "$fixture_dir/events"
          echo "$fixture_id" > "$fixture_dir/starting"
          read -r permit
          echo live > "$fixture_dir/live"
          ;;
        --show-stats) echo 'Cache location Local disk: fixture' ;;
      esac
    }
    echo READY
    read -r start
    ct_configure_sccache || exit $?
    echo DONE
  `, `cache-${i}`));
  const observers = [];
  try {
    await Promise.all(clients.map(c => c.ready));
    const starting = observed(directory, () => fs.existsSync(path.join(directory, 'starting')) && fs.readFileSync(path.join(directory, 'starting'), 'utf8').trim());
    for (const c of clients) c.child.stdin.write('start\n');
    const starter = await starting;
    assert.equal(fs.existsSync(path.join(directory, 'state', 'state.lock')), false);
    const observer = shell(directory, statusBody);
    observers.push(observer);
    const status = await observer.exited;
    assert.equal(status.code, 0, status.errors);
    assert.equal(JSON.parse(status.output).state, 'idle');
    clients.find(c => c.id === starter).child.stdin.end('permit\n');
    const results = await Promise.all(clients.map(c => c.exited));
    for (const result of results) {
      assert.equal(result.code, 0, result.errors);
      assert.match(result.output, /DONE/);
      assert.doesNotMatch(result.errors, /10048|EPERM|Timed out/);
    }
    assert.deepEqual(fs.readFileSync(path.join(directory, 'events'), 'utf8').trim().split(/\r?\n/), ['stop', 'start']);
  } finally { await cleanup(directory, [...clients, ...observers]); }
});

test('cache retention holds its own mutex while gateway metadata stays available', { timeout: 45000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-retention-'));
  const holder = shell(directory, `
    source scripts/cache-policy.sh
    source scripts/build-gateway.sh
    ct_gateway_set_fixture_dir "$1/state"
    node() {
      case "$1" in
        */cache-retention.mjs) echo HOLDING; read -r permit ;;
        *) command node "$@" ;;
      esac
    }
    ct_cache_retention auto
  `);
  const observers = [];
  try {
    assert.equal(await holder.ready, 'HOLDING');
    assert.equal(fs.existsSync(path.join(directory, 'state', 'cache-retention-state', 'state.lock')), true);
    assert.equal(fs.existsSync(path.join(directory, 'state', 'state.lock')), false);
    const observer = shell(directory, statusBody);
    observers.push(observer);
    const result = await observer.exited;
    assert.equal(result.code, 0, result.errors);
    assert.equal(JSON.parse(result.output).state, 'idle');
    holder.child.stdin.end('permit\n');
    assert.equal((await holder.exited).code, 0);
  } finally { await cleanup(directory, [holder, ...observers]); }
});

test('gateway helpers stay pinned to the launching scripts after checkout cwd changes', { timeout: 20000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-helper-root-'));
  const client = shell(directory, `
    source scripts/build-gateway.sh
    mkdir -p "$1/state"
    cd "$1"
    ct_repo_root() { pwd; }
    ct_gateway_mutex_acquire "$1/state" || exit $?
    ct_gateway_mutex_release "$1/state" || exit $?
    ct_gateway_set_fixture_dir "$1/state"
    ct_gateway_status
  `);
  try {
    const result = await client.exited;
    assert.equal(result.code, 0, result.errors);
    assert.equal(JSON.parse(result.output).state, 'idle');
  } finally { await cleanup(directory, [client]); }
});

test('cache reset metadata failure stops server restart before compile', { timeout: 15000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-cache-evidence-'));
  const client = shell(directory, `
    source scripts/cache-policy.sh
    source scripts/build-gateway.sh
    ct_gateway_set_fixture_dir "$1/state"
    CT_GATEWAY_ACTIVE=1
    CT_BUILD_JOB_ID=reset-failure
    CT_SCCACHE_STORAGE=local
    pgrep() { return 1; }
    powershell.exe() { return 1; }
    ct_gateway_state_tool() { return 1; }
    sccache() { echo STOPPED; }
    ct_sccache_start_with_evidence() { echo STARTED; }
    if ct_sccache_restart; then echo COMPILE_STARTED; else exit "$?"; fi
  `);
  try {
    const result = await client.exited;
    assert.equal(result.code, 74, result.errors);
    assert.doesNotMatch(result.output, /STOPPED|STARTED/);
    assert.match(result.errors, /metadata write failed before compilation: cache reset evidence/);
    assert.equal(fs.existsSync(path.join(directory, 'state', 'sccache-start-state', 'state.lock')), false);
  } finally { await cleanup(directory, [client]); }
});
