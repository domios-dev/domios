import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { orphanLanePaths } from '../orphan-lane-units.mjs';

const DAY = 86400000;

test('removed lanes lose their units and unshared incremental sessions; live and unknown units survive', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'orphan-lane-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const unit = (name, hash, lanes, age = 4) => {
    const dir = path.join(root, 'debug/.fingerprint', `${name}-${hash}`);
    const deps = path.join(root, 'debug/deps');
    const session = path.join(root, 'debug/incremental', `${name.replaceAll('-', '_')}-abc123`);
    for (const directory of [dir, deps, session]) fs.mkdirSync(directory, { recursive: true });
    fs.writeFileSync(path.join(dir, 'lib.json'), JSON.stringify({ path: hash }));
    fs.writeFileSync(path.join(deps, `${name.replaceAll('-', '_')}-${hash}.d`), lanes.map(lane => `${lane}/src/lib.rs:\n`).join(''));
    fs.writeFileSync(path.join(deps, `lib${name.replaceAll('-', '_')}-${hash}.rlib`), 'payload');
    for (const file of [path.join(dir, 'lib.json'), dir, session]) fs.utimesSync(file, new Date(Date.now() - age * DAY), new Date(Date.now() - age * DAY));
    return { dir, session };
  };
  const gone = '/machine/project-worktrees/gone';
  const live = '/machine/project-worktrees/live';
  const orphan = unit('only-gone', '1'.repeat(16), [gone]);
  const sharedGone = unit('shared', '2'.repeat(16), [gone]);
  const sharedLive = unit('shared', '3'.repeat(16), [live]);
  const mixed = unit('mixed', '4'.repeat(16), [gone, live]);
  const unknown = unit('unknown', '5'.repeat(16), []);
  const recent = unit('recent', '6'.repeat(16), [gone], 1);
  const windows = unit('windows', '7'.repeat(16), ['C\\:/project space/project-worktrees/gone']);
  const relative = unit('relative', '8'.repeat(16), ['crates\\codeterm-orchestration']);
  const relativeLane = unit('relative-lane', '9'.repeat(16), ['..\\project-worktrees\\gone']);
  const paths = orphanLanePaths(root, { exists: lane => lane === live });
  for (const dir of [orphan.dir, orphan.session, sharedGone.dir, windows.dir]) assert.ok(paths.includes(dir), dir);
  for (const dir of [sharedGone.session, sharedLive.dir, mixed.dir, unknown.dir, recent.dir, relative.dir, relative.session, relativeLane.dir]) assert.ok(!paths.includes(dir), dir);
  assert.ok(paths.some(file => file.endsWith(`libonly_gone-${'1'.repeat(16)}.rlib`)));
});
