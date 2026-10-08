import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { candidate, mark, retain, physical } from './cache-retention.mjs';

const DAY = 86400000;
const now = Date.now();
const fixtureParent = path.resolve('.research/build-perf');
fs.mkdirSync(fixtureParent, { recursive: true });

function fixture(t) {
  const base = fs.mkdtempSync(path.join(fixtureParent, 'retention-'));
  const gateway = path.join(base, 'build-gateway');
  const root = path.join(gateway, 'targets', 'exceptional');
  const checkout = path.join(base, 'checkout');
  const other = path.join(base, 'other');
  for (const dir of [root, checkout, other]) fs.mkdirSync(dir, { recursive: true });
  t.after(() => fs.rmSync(base, { recursive: true, force: true }));
  const create = (name, age, repo = other, owner = 0) => {
    const target = path.join(root, name);
    fs.mkdirSync(target);
    fs.writeFileSync(path.join(target, 'artifact'), Buffer.alloc(1024));
    mark(gateway, repo, target, now - age * DAY, owner);
    return target;
  };
  const run = (options = {}) => retain(gateway, checkout, { now, sweep: false, ...options });
  return { base, gateway, root, checkout, other, create, run };
}

test('marker records physical checkout, use time and size', (t) => {
  const f = fixture(t);
  const target = f.create('marked', 1);
  const marker = JSON.parse(fs.readFileSync(path.join(target, '.codeterm-cache.json')));
  assert.equal(marker.checkout, physical(f.other));
  assert.equal(marker.lastUsed, now - DAY);
  assert.equal(marker.sizeBytes, 1024);
});

test('three-day expiry and removed checkouts are pruned; recent targets survive', (t) => {
  const f = fixture(t);
  const old = f.create('old', 4);
  const recent = f.create('recent', 2);
  const missing = f.create('missing', 0, f.checkout);
  const markerPath = path.join(missing, '.codeterm-cache.json');
  const marker = JSON.parse(fs.readFileSync(markerPath));
  marker.checkout = path.join(f.base, 'removed-checkout');
  fs.writeFileSync(markerPath, JSON.stringify(marker));
  f.run();
  assert.equal(fs.existsSync(old), false);
  assert.equal(fs.existsSync(missing), false);
  assert.equal(fs.existsSync(recent), true);
});

test('expiry is configurable and unmarked targets receive two-day grace', (t) => {
  const f = fixture(t);
  const marked = f.create('marked', 2);
  const old = path.join(f.root, 'legacy-old');
  const fresh = path.join(f.root, 'legacy-fresh');
  fs.mkdirSync(old);
  fs.mkdirSync(fresh);
  fs.utimesSync(old, new Date(now - 3 * DAY), new Date(now - 3 * DAY));
  fs.utimesSync(fresh, new Date(now - DAY), new Date(now - DAY));
  f.run({ maxAgeDays: 1 });
  assert.equal(fs.existsSync(marked), false);
  assert.equal(fs.existsSync(old), false);
  assert.equal(fs.existsSync(fresh), true);
});

test('cap evicts least recently used after pruning stale targets', (t) => {
  const f = fixture(t);
  const oldest = f.create('oldest', 2);
  const newest = f.create('newest', 1);
  const stale = f.create('stale', 4);
  const keepSize = fs.statSync(path.join(newest, '.codeterm-cache.json')).size + 1024;
  f.run({ capBytes: keepSize });
  assert.equal(fs.existsSync(oldest), false);
  assert.equal(fs.existsSync(stale), false);
  assert.equal(fs.existsSync(newest), true);
});

test('current checkout and a live building owner are protected even over cap', (t) => {
  const f = fixture(t);
  const current = f.create('current', 8, f.checkout);
  const building = f.create('building', 8, f.other, process.pid);
  const sibling = f.create('same-building-checkout', 8);
  const idleRepo = path.join(f.base, 'idle-checkout');
  fs.mkdirSync(idleRepo);
  const idle = f.create('idle', 8, idleRepo);
  f.run({ capBytes: 0 });
  assert.equal(fs.existsSync(current), true);
  assert.equal(fs.existsSync(building), true);
  assert.equal(fs.existsSync(sibling), true);
  assert.equal(fs.existsSync(idle), false);
});

test('dry-run changes neither artifacts nor retention timestamp', (t) => {
  const f = fixture(t);
  const old = f.create('old', 4);
  assert.ok(f.run({ dryRun: true }) >= 1024);
  assert.equal(fs.existsSync(old), true);
  assert.equal(fs.existsSync(path.join(f.gateway, 'cache-retention.json')), false);
});

test('automatic retention scans at most hourly and manual gc bypasses throttle', (t) => {
  const f = fixture(t);
  f.run({ automatic: true });
  const old = f.create('old', 4);
  f.run({ automatic: true, now: now + 3599999 });
  assert.equal(fs.existsSync(old), true);
  f.run();
  assert.equal(fs.existsSync(old), false);
});

test('empty, root, parent, outside and junction candidates are refused', (t) => {
  const f = fixture(t);
  for (const unsafe of ['', path.parse(f.root).root, f.root, path.dirname(f.root), f.other]) {
    assert.throws(() => candidate(f.root, unsafe));
  }
  const outside = path.join(f.other, 'keep');
  fs.writeFileSync(outside, 'untouched');
  const link = path.join(f.root, 'junction');
  fs.symlinkSync(f.other, link, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => candidate(f.root, link), /link|junction/);
  f.run({ capBytes: 0 });
  assert.equal(fs.readFileSync(outside, 'utf8'), 'untouched');
});

test('nested links refuse deletion', (t) => {
  const f = fixture(t);
  const target = f.create('nested', 4);
  fs.symlinkSync(f.other, path.join(target, 'redirect'), process.platform === 'win32' ? 'junction' : 'dir');
  f.run({ capBytes: 0 });
  assert.equal(fs.existsSync(target), true);
  assert.equal(fs.existsSync(f.other), true);
});

test('sweep uses configurable age, exact checkout targets and propagates dry-run', (t) => {
  const f = fixture(t);
  for (const relative of ['src-tauri/target', 'cli/target']) {
    fs.mkdirSync(path.join(f.checkout, relative), { recursive: true });
  }
  const calls = [];
  f.run({ sweep: true, dryRun: true, maxAgeDays: 5, runner: (command, args, options) => {
    calls.push({ command, args, options });
    return { status: 0 };
  } });
  const sweep = calls.filter(call => call.command === 'cargo');
  assert.equal(sweep.length, 2);
  for (const call of sweep) {
    assert.deepEqual(call.args, ['sweep', '--time', '5', '--dry-run']);
    assert.equal(call.options.env.CARGO_TARGET_DIR, path.join(call.options.cwd, 'target'));
    assert.equal(path.dirname(call.options.cwd), physical(f.checkout));
  }
});

test('sweep refuses redirected checkout targets and handles missing tools without failing', (t) => {
  const f = fixture(t);
  fs.mkdirSync(path.join(f.checkout, 'src-tauri'));
  fs.symlinkSync(f.other, path.join(f.checkout, 'src-tauri/target'),
    process.platform === 'win32' ? 'junction' : 'dir');
  let cargoCalls = 0;
  f.run({ sweep: true, runner: (command) => {
    if (command === 'cargo') cargoCalls++;
    return { status: 0 };
  } });
  assert.equal(cargoCalls, 0);
  assert.doesNotThrow(() => f.run({ sweep: true, runner: () => ({ status: null }) }));
});
