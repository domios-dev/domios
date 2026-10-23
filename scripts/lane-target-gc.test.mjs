import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { trimLaneTargets } from './lane-target-gc.mjs';

const DAY = 86400000;
const now = 10 * DAY;

function fixture(t) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'lane-target-gc-'));
  const canonical = path.join(base, 'project');
  fs.mkdirSync(canonical);
  const root = `${canonical}-worktrees`;
  fs.mkdirSync(root);
  t.after(() => fs.rmSync(base, { recursive: true, force: true }));
  const target = (name, age, relative = 'target') => {
    const lane = path.join(root, name);
    const output = path.join(lane, relative);
    fs.mkdirSync(output, { recursive: true });
    fs.writeFileSync(path.join(output, 'artifact'), 'build output');
    stamp(output, now - age * DAY);
    return output;
  };
  const run = options => trimLaneTargets(canonical, { now, runner: (program, args, options) => program === 'git' ? { status: 0, stdout: '' } : spawnSync(program, args, options), log: () => {}, ...options });
  return { base, canonical, root, target, run };
}

function stamp(directory, time) {
  for (const entry of fs.readdirSync(directory)) {
    const child = path.join(directory, entry);
    if (fs.lstatSync(child).isDirectory()) stamp(child, time);
    else fs.utimesSync(child, new Date(time), new Date(time));
  }
  fs.utimesSync(directory, new Date(time), new Date(time));
}

test('lane target GC removes expired outputs, keeps fresh artifacts and source', t => {
  const f = fixture(t);
  const old = ['target', 'src-tauri/target', 'cli/target'].map((relative, i) => f.target(`old-${i}`, 4, relative));
  const recent = f.target('recent', 2);
  const freshArtifact = f.target('old-directory', 4);
  fs.utimesSync(path.join(freshArtifact, 'artifact'), new Date(now), new Date(now));
  const source = path.join(f.root, 'old-0', 'main.rs');
  fs.writeFileSync(source, 'source');
  assert.equal(f.run({}), 36);
  for (const output of old) assert.equal(fs.existsSync(output), false);
  assert.equal(fs.existsSync(recent), true);
  assert.equal(fs.existsSync(freshArtifact), true);
  assert.equal(fs.readFileSync(source, 'utf8'), 'source');
});

test('lane target GC respects preview, current checkout, retention and tracked content', t => {
  const f = fixture(t);
  const output = f.target('old', 4);
  f.run({ dryRun: true });
  assert.equal(fs.existsSync(output), true);
  f.run({ checkout: path.dirname(output) });
  assert.equal(fs.existsSync(output), true);
  f.run({ maxAgeDays: 5 });
  assert.equal(fs.existsSync(output), true);
  f.run({ runner: () => ({ status: 0, stdout: 'target/owned.rs\0' }) });
  assert.equal(fs.existsSync(output), true);
});

test('lane target GC protects output junctions and linked lanes', t => {
  const f = fixture(t);
  const output = f.target('old', 4);
  const outside = path.join(f.base, 'outside');
  fs.mkdirSync(outside);
  fs.writeFileSync(path.join(outside, 'proof'), 'preserved');
  const link = path.join(output, 'link');
  fs.symlinkSync(outside, link, process.platform === 'win32' ? 'junction' : 'dir');
  fs.lutimesSync(link, new Date(now - 4 * DAY), new Date(now - 4 * DAY));
  fs.utimesSync(output, new Date(now - 4 * DAY), new Date(now - 4 * DAY));
  fs.symlinkSync(outside, path.join(f.root, 'linked-lane'), process.platform === 'win32' ? 'junction' : 'dir');
  f.run({});
  assert.equal(fs.existsSync(output), true);
  assert.equal(fs.readFileSync(path.join(outside, 'proof'), 'utf8'), 'preserved');
});
