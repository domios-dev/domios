// Caller: scripts/check-build-gateway-policy.sh
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { test } from 'node:test';
import { mapCheckouts, orphanCandidates, worktrees } from './cargo-build-gc.mjs';

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
