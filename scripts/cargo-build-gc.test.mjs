// Caller: scripts/check-build-gateway-policy.sh
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { test } from 'node:test';
import { mapCheckouts, orphanCandidates, trimBuildDirectories, worktrees } from './cargo-build-gc.mjs';

const python = ['python3', 'python'].find((name) => spawnSync(name, ['--version']).status === 0);
const helper = path.resolve('scripts/cargo-build-gc.py');

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cargo-gc-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const create = (prefix, suffix) => {
    const dir = path.join(root, prefix, suffix.repeat(14));
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'artifact'), 'payload');
    return dir;
  };
  const run = (dir, dry = false) => {
    const result = spawnSync(python, [helper, root, dir, ...(dry ? ['--dry-run'] : [])], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  return { root, create, run };
}

test('porcelain worktree paths preserve spaces and include detached checkouts', () => {
  assert.deepEqual(worktrees('repo', () => ({ status: 0, stdout: 'worktree C:/one space\0HEAD abc\0\0worktree C:/two\0detached\0' })),
    ['C:/one space', 'C:/two']);
});

test('Cargo metadata maps separate workspaces and deduplicates member manifests', (t) => {
  const f = fixture(t);
  const seen = [];
  const mappings = mapCheckouts([f.root], { manifests: () => ['cli/Cargo.toml', 'src-tauri/Cargo.toml', 'src-tauri/crates/x/Cargo.toml'],
    runner: (_program, args) => {
      seen.push(args);
      const workspace = path.dirname(args.at(-1));
      return { status: 0, stdout: JSON.stringify({ workspace_root: workspace, build_directory: path.join(f.root, 'ab', '0'.repeat(14)) }) };
    } });
  assert.equal(mappings.length, 2);
  assert.equal(seen.length, 2);
  assert.ok(seen.every((args) => args.includes('--offline') && args.includes('--no-deps')));
});

test('missing metadata field and failed metadata refuse collection', (t) => {
  const f = fixture(t);
  for (const result of [{ status: 0, stdout: '{}' }, { status: 1, stderr: 'offline resolution failed' }]) {
    assert.throws(() => mapCheckouts([f.root], { manifests: () => ['Cargo.toml'], runner: () => result }));
  }
});

test('live checkouts including old caches and stable gateway remain protected', (t) => {
  const f = fixture(t);
  const live = f.create('ab', '1');
  const gateway = f.create('cd', '2');
  const orphan = f.create('ef', '3');
  fs.utimesSync(live, new Date(0), new Date(0));
  assert.deepEqual(orphanCandidates(f.root, [{ build: live }, { build: gateway }]), [orphan]);
});

test('dry run preserves artifacts; real collection removes only selected orphan', (t) => {
  const f = fixture(t);
  const dir = f.create('ab', '1');
  const live = f.create('cd', '2');
  assert.equal(f.run(dir, true).bytes, 7);
  assert.equal(fs.readFileSync(path.join(dir, 'artifact'), 'utf8'), 'payload');
  assert.equal(f.run(dir).bytes, 7);
  assert.equal(fs.existsSync(dir), false);
  assert.equal(fs.existsSync(live), true);
});

test('Cargo-compatible held lock protects an orphan; released lock allows removal', async (t) => {
  const f = fixture(t);
  const dir = f.create('ab', '1');
  const lock = path.join(dir, 'debug', '.cargo-lock');
  fs.mkdirSync(path.dirname(lock));
  fs.writeFileSync(lock, '');
  const code = 'import importlib.util,sys; s=importlib.util.spec_from_file_location("gc",sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m)\nwith m.cargo_lock(sys.argv[2]):\n print("ready",flush=True)\n sys.stdin.read(1)';
  const child = spawn(python, ['-c', code, helper, lock], { stdio: ['pipe', 'pipe', 'inherit'] });
  t.after(() => child.kill());
  const exited = once(child, 'exit');
  await once(child.stdout, 'data');
  assert.equal(f.run(dir, true).status, 'protected');
  assert.equal(f.run(dir).status, 'protected');
  assert.equal(fs.existsSync(dir), true);
  child.stdin.end('x');
  assert.equal((await exited)[0], 0);
  assert.equal(f.run(dir).status, 'removed orphan');
});

test('links and paths outside the hash root refuse deletion', (t) => {
  const f = fixture(t);
  const dir = f.create('ab', '1');
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), 'cargo-gc-outside-'));
  t.after(() => fs.rmSync(outside, { recursive: true, force: true }));
  fs.writeFileSync(path.join(outside, 'keep'), 'keep');
  fs.symlinkSync(outside, path.join(dir, 'redirect'), process.platform === 'win32' ? 'junction' : 'dir');
  const result = spawnSync(python, [helper, f.root, dir], { encoding: 'utf8' });
  assert.equal(result.status, 2);
  assert.equal(fs.readFileSync(path.join(outside, 'keep'), 'utf8'), 'keep');
  assert.equal(spawnSync(python, [helper, f.root, outside]).status, 2);
});

function trimFixture(t, shared = false) {
  const f = fixture(t);
  const dir = shared ? path.join(f.root, typeof shared === 'string' ? shared : 'lanes') : f.create('ab', '1');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'CACHEDIR.TAG'), 'Signature: 8a477f597d28d172789f06886806bc55');
  const stale = path.join(dir, 'debug', 'deps', 'libold-aaaaaaaaaaaaaaaa.rlib');
  const live = path.join(dir, 'debug', 'deps', 'libold-bbbbbbbbbbbbbbbb.rlib');
  const session = path.join(dir, 'debug', 'incremental', 'old-1x2y');
  fs.mkdirSync(path.dirname(stale), { recursive: true });
  fs.mkdirSync(session, { recursive: true });
  for (const file of [stale, live, path.join(session, 'dep-graph.bin'), path.join(dir, 'debug', '.cargo-lock')]) fs.writeFileSync(file, 'x');
  const trim = (paths, dry = false) => {
    const list = path.join(f.root, 'paths.json');
    fs.writeFileSync(list, JSON.stringify(paths));
    return spawnSync(python, [helper, '--trim', f.root, dir, list, ...(dry ? ['--dry-run'] : [])], { encoding: 'utf8' });
  };
  return { ...f, dir, stale, live, session, trim };
}

test('trim removes exactly the listed variants under the Cargo lock; dry run keeps them', (t) => {
  const f = trimFixture(t);
  const dry = f.trim([f.stale, f.session], true);
  assert.equal(dry.status, 0, dry.stderr);
  assert.equal(JSON.parse(dry.stdout).status, 'would trim');
  assert.equal(fs.existsSync(f.stale) && fs.existsSync(f.session), true);
  const real = f.trim([f.stale, f.session]);
  assert.equal(JSON.parse(real.stdout).status, 'trimmed');
  assert.deepEqual([fs.existsSync(f.stale), fs.existsSync(f.session), fs.existsSync(f.live)], [false, false, true]);
  assert.equal(fs.existsSync(path.join(f.dir, 'debug', '.cargo-lock')), true);
});

test('trim refuses paths outside the hash directory and Cargo bookkeeping files', (t) => {
  const f = trimFixture(t);
  const outside = path.join(f.root, 'keep');
  fs.writeFileSync(outside, 'keep');
  assert.equal(f.trim([outside]).status, 2);
  assert.equal(f.trim([path.join(f.dir, 'debug', '.cargo-lock')]).status, 2);
  assert.equal(fs.existsSync(outside), true);
});

test('shared lane directory is trimmed once across worktrees and keeps live units', (t) => {
  const f = trimFixture(t, true);
  const mappings = [{ build: f.dir }, { build: f.dir }];
  let scans = 0;
  const plan = () => { scans++; return [f.stale]; };
  const env = { CODETERM_RUST_CACHE_ROOT: f.root };
  assert.equal(trimBuildDirectories(mappings, { plan, env, dryRun: true }), 1);
  assert.equal(scans, 1);
  assert.equal(fs.existsSync(f.stale), true);
  assert.equal(trimBuildDirectories(mappings, { plan, env }), 1);
  assert.equal(fs.existsSync(f.stale), false);
  assert.equal(fs.existsSync(f.live), true);
  assert.equal(f.trim([path.join(f.dir, 'CACHEDIR.TAG')]).status, 2);
  fs.unlinkSync(path.join(f.dir, 'CACHEDIR.TAG'));
  assert.equal(f.trim([f.live]).status, 2);
});

for (const [variable, directory] of [['CODETERM_RUST_CACHE_ROOT', 'lanes'], ['CARGO_HOME', 'codeterm-lanes']]) {
  test(`retention discovers ${variable} lane cache without a Cargo metadata mapping`, (t) => {
    const f = trimFixture(t, directory);
    const canonicalTarget = path.join(f.root, 'canonical', 'target');
    fs.mkdirSync(canonicalTarget, { recursive: true });
    fs.writeFileSync(path.join(canonicalTarget, 'keep'), 'canonical');
    const scanned = [];
    const bytes = trimBuildDirectories([{ build: canonicalTarget }], {
      env: { [variable]: f.root },
      plan: (build) => { scanned.push(build); return [f.stale]; },
    });
    assert.equal(bytes, 1);
    assert.deepEqual(scanned, [f.dir]);
    assert.equal(fs.existsSync(f.stale), false);
    assert.equal(fs.existsSync(f.live), true);
    assert.equal(fs.readFileSync(path.join(canonicalTarget, 'keep'), 'utf8'), 'canonical');
  });
}

test('a held Cargo lock protects the whole build directory from trim', async (t) => {
  const f = trimFixture(t, true);
  const lock = path.join(f.dir, 'debug', '.cargo-lock');
  const code = 'import importlib.util,sys; s=importlib.util.spec_from_file_location("gc",sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m)\nwith m.cargo_lock(sys.argv[2]):\n print("ready",flush=True)\n sys.stdin.read(1)';
  const child = spawn(python, ['-c', code, helper, lock], { stdio: ['pipe', 'pipe', 'inherit'] });
  t.after(() => child.kill());
  const exited = once(child, 'exit');
  await once(child.stdout, 'data');
  assert.equal(JSON.parse(f.trim([f.stale]).stdout).status, 'protected');
  assert.equal(fs.existsSync(f.stale), true);
  child.stdin.end('x');
  assert.equal((await exited)[0], 0);
  assert.equal(JSON.parse(f.trim([f.stale]).stdout).status, 'trimmed');
});
