import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import test from 'node:test';

const python = ['python3', 'python'].find(name => spawnSync(name, ['--version']).status === 0);
const helper = path.resolve('scripts/cargo-build-gc.py');
const now = Date.now();

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'incremental-retention-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const build = path.join(root, 'ab', '1'.repeat(14));
  const incremental = path.join(build, 'debug', 'incremental');
  fs.mkdirSync(incremental, { recursive: true });
  fs.writeFileSync(path.join(build, 'CACHEDIR.TAG'), 'Signature: 8a477f597d28d172789f06886806bc55');
  const cargoLock = path.join(build, 'debug', '.cargo-lock');
  fs.writeFileSync(cargoLock, '');
  const age = (file, hours) => fs.utimesSync(file, new Date(now - hours * 3600000), new Date(now - hours * 3600000));
  const session = (variant, name, hours) => {
    const directory = path.join(incremental, variant, name);
    fs.mkdirSync(directory, { recursive: true });
    const payload = path.join(directory, 'dep-graph.bin');
    fs.writeFileSync(payload, '12345');
    age(payload, hours);
    age(directory, hours);
    age(path.dirname(directory), hours);
    return directory;
  };
  const run = (dry = false, env = {}) => {
    const result = spawnSync(python, [helper, '--incremental', root, build, ...(dry ? ['--dry-run'] : [])],
      { encoding: 'utf8', env: { ...process.env, CODETERM_INCREMENTAL_IDLE_HOURS: '12', ...env } });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  return { root, build, incremental, cargoLock, age, session, run };
}

test('keeps newest hash and session per crate, protecting recent overflow and reporting bytes', t => {
  const f = fixture(t);
  const oldHash = f.session('internal_crate-aaa', 's-old', 30);
  const oldSession = f.session('internal_crate-bbb', 's-old', 28);
  const newest = f.session('internal_crate-bbb', 's-new', 0);
  const recent = f.session('internal_crate-ccc', 's-recent', 1);
  const other = f.session('another_crate-aaa', 's-only', 40);
  const dry = f.run(true);
  assert.equal(dry.bytes, 10);
  assert.ok(fs.existsSync(oldHash) && fs.existsSync(oldSession));
  const applied = f.run();
  assert.equal(applied.bytes, 10);
  assert.equal(fs.existsSync(path.dirname(oldHash)), false);
  assert.equal(fs.existsSync(oldSession), false);
  assert.ok([newest, recent, other].every(fs.existsSync));
});

test('descendant write time protects an old hash and idle cap is configurable', t => {
  const f = fixture(t);
  const old = f.session('crate-aaa', 's-old', 30);
  f.session('crate-bbb', 's-new', 1);
  f.age(path.join(old, 'dep-graph.bin'), 2);
  assert.equal(f.run().bytes, 0);
  assert.equal(f.run(false, { CODETERM_INCREMENTAL_IDLE_HOURS: '1' }).bytes, 5);
});

for (const kind of ['cargo', 'session', 'lane']) {
  test(`held ${kind} lock protects cache; released lock permits trimming`, async t => {
    const f = fixture(t);
    const old = f.session('crate-aaa', 's-old', 30);
    f.session('crate-bbb', 's-new', 20);
    let lock = f.cargoLock;
    if (kind === 'session') {
      lock = path.join(path.dirname(old), 's-old.lock');
      fs.writeFileSync(lock, '');
      f.age(lock, 30);
      f.age(path.dirname(old), 30);
    }
    if (kind === 'lane') {
      const lane = path.join(f.root, 'lanes');
      fs.renameSync(f.build, lane);
      f.build = lane;
      lock = path.join(lane, '.domios-lane-check.lock');
      fs.writeFileSync(lock, '');
    }
    const code = 'import importlib.util,sys; s=importlib.util.spec_from_file_location("gc",sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m)\nwith m.cargo_lock(sys.argv[2]):\n print("ready",flush=True)\n sys.stdin.read(1)';
    const child = spawn(python, ['-c', code, helper, lock], { stdio: ['pipe', 'pipe', 'inherit'] });
    t.after(() => child.kill());
    const exited = once(child, 'exit');
    await once(child.stdout, 'data');
    const run = () => {
      const result = spawnSync(python, [helper, '--incremental', f.root, f.build], { encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      return JSON.parse(result.stdout);
    };
    assert.equal(run().bytes, 0);
    child.stdin.end('x');
    assert.equal((await exited)[0], 0);
    assert.equal(run().bytes, 5);
  });
}

test('outside paths, nested junctions and missing cache marker refuse deletion', t => {
  const f = fixture(t);
  const old = f.session('crate-aaa', 's-old', 30);
  f.session('crate-bbb', 's-new', 20);
  const outside = path.join(f.root, 'outside');
  fs.mkdirSync(outside);
  const keep = path.join(outside, 'keep');
  fs.writeFileSync(keep, 'untouched');
  fs.symlinkSync(outside, path.join(old, 'redirect'), process.platform === 'win32' ? 'junction' : 'dir');
  for (const directory of [f.build, outside]) {
    assert.equal(spawnSync(python, [helper, '--incremental', f.root, directory]).status, 2);
  }
  assert.equal(fs.readFileSync(keep, 'utf8'), 'untouched');
  fs.unlinkSync(path.join(old, 'redirect'));
  fs.unlinkSync(path.join(f.build, 'CACHEDIR.TAG'));
  assert.equal(spawnSync(python, [helper, '--incremental', f.root, f.build]).status, 2);
  assert.ok(fs.existsSync(old));
});

test('checkout targets and target-triple profiles use the same policy', t => {
  const f = fixture(t);
  f.session('crate-aaa', 's-old', 30);
  f.session('crate-bbb', 's-new', 20);
  const target = path.join(f.root, 'target');
  fs.renameSync(f.build, target);
  const triple = path.join(target, 'aarch64-apple-darwin');
  fs.mkdirSync(triple);
  fs.renameSync(path.join(target, 'debug'), path.join(triple, 'debug'));
  const result = spawnSync(python, [helper, '--incremental', f.root, target], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).bytes, 5);
});

test('cache policy enables incremental trimming only for the gateway owner', t => {
  const f = fixture(t);
  for (const owned of [0, 1]) {
    const result = spawnSync('bash', ['-c', `
      source scripts/cache-policy.sh
      ct_gateway_dir() { echo "$TEST_GATEWAY"; }
      ct_cache_home() { echo "$TEST_GATEWAY"; }
      ct_gateway_owned_by_caller() { test "$TEST_OWNED" = 1; }
      ct_gateway_mutex_acquire() { :; }
      ct_gateway_mutex_release() { :; }
      node() { echo "allowed=$CT_CACHE_INCREMENTAL_ALLOWED"; }
      ct_cache_retention auto
    `], { encoding: 'utf8', env: { ...process.env, TEST_GATEWAY: f.root.replaceAll('\\', '/'), TEST_OWNED: String(owned) } });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, new RegExp(`allowed=${owned}`));
  }
});
