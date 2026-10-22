import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { gatewayCandidates, trimGatewayCaches } from '../gateway-cache-gc.mjs';
import { FULL_LANES, testTargetDir } from '../rust-test-lanes.mjs';

const DAY = 86400000;

function fixture(t) {
  const gateway = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-cache-gc-'));
  t.after(() => fs.rmSync(gateway, { recursive: true, force: true }));
  const create = (name, age = 4) => {
    const dir = path.join(gateway, name);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'artifact'), 'payload');
    for (const file of [path.join(dir, 'artifact'), dir]) fs.utimesSync(file, new Date(Date.now() - age * DAY), new Date(Date.now() - age * DAY));
    return dir;
  };
  return { gateway, create };
}

test('expired gateway leftovers report exact bytes and dry run preserves them', t => {
  const f = fixture(t);
  const old = ['broken', 'pending', 'previous'].map(kind => f.create(`check-worktree.${kind}-old`));
  const fresh = f.create('check-worktree.pending-fresh', 1);
  const canonical = f.create('check-worktree');
  const sccache = f.create('sccache');
  const run = dryRun => trimGatewayCaches(f.gateway, { dryRun, env: {}, log: () => {} });
  assert.equal(run(true), 21);
  assert.ok(old.every(dir => fs.existsSync(dir)));
  assert.equal(run(false), 21);
  assert.ok(old.every(dir => !fs.existsSync(dir)));
  for (const dir of [fresh, canonical, sccache]) assert.ok(fs.existsSync(dir));
});

test('active job, publication journal and all current target modes are protected', t => {
  const f = fixture(t);
  const pending = f.create('check-worktree.pending-active');
  const broken = f.create('check-worktree.broken-running');
  fs.mkdirSync(path.join(f.gateway, 'lock'));
  fs.writeFileSync(path.join(f.gateway, 'lock/job_id'), 'running');
  fs.writeFileSync(path.join(f.gateway, 'check-worktree-publication.json'), JSON.stringify({ staging: pending }));
  const env = { CI_TARGET_ROOT: f.create('targets') };
  for (const lane of FULL_LANES) for (const mode of ['full', 'platform', 'embeddings']) fs.mkdirSync(testTargetDir(lane, { env, mode }), { recursive: true });
  fs.mkdirSync(path.join(env.CI_TARGET_ROOT, 'cli'));
  const candidates = gatewayCandidates(f.gateway, { env });
  assert.deepEqual(candidates.map(item => item.dir), [path.join(env.CI_TARGET_ROOT, 'cli')]);
  assert.ok(fs.existsSync(broken));
});

test('junctions and corrupt publication metadata retain leftovers', t => {
  const f = fixture(t);
  const outside = f.create('outside');
  fs.symlinkSync(outside, path.join(f.gateway, 'check-worktree.broken-link'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.equal(trimGatewayCaches(f.gateway, { env: {}, log: () => {} }), 0);
  assert.ok(fs.existsSync(path.join(outside, 'artifact')));
  fs.writeFileSync(path.join(f.gateway, 'check-worktree-publication.json'), 'broken');
  assert.throws(() => gatewayCandidates(f.gateway, { env: {} }));
});
