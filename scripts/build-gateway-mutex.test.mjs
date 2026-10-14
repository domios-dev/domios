// Caller: skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { nativeIdentity } from './build-gateway-guard.mjs';
import { once } from 'node:events';
import readline from 'node:readline';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '..');
function contender(directory, timeout = 700) {
  const child = spawn('bash', ['-c', `
    source scripts/build-gateway.sh
    ct_repo_root() { printf '%s\\n' "$PWD"; }
    CT_BUILD_GATEWAY_SCOPE=fixture
    CT_GATEWAY_MUTEX_TIMEOUT_MS="$2"
    echo READY
    read -r start
    if ct_gateway_mutex_acquire "$1"; then
      echo OWNER
      read -r release
      ct_gateway_mutex_release "$1"
    else
      rc=$?
      echo REFUSED
      exit "$rc"
    fi
  `, 'fixture', directory, String(timeout)], { cwd: root, stdio: ['pipe', 'pipe', 'pipe'] });
  let errors = '';
  child.stderr.on('data', chunk => errors += chunk);
  const lines = readline.createInterface({ input: child.stdout });
  const ready = new Promise(resolve => lines.once('line', resolve));
  const outcome = new Promise(resolve => lines.on('line', line => {
    if (line === 'OWNER' || line === 'REFUSED') resolve(line);
  }));
  const exited = once(child, 'exit').then(([code]) => ({ code, errors }));
  return { child, ready, outcome, exited };
}

for (const mode of ['fresh', 'incomplete']) {
  test(`${mode}: 20 rounds of six real competing Bash owners`, { timeout: 60000 }, async () => {
    for (let round = 0; round < 20; round++) {
      const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-contention-'));
      const lock = path.join(directory, 'state.lock');
      if (mode === 'incomplete') fs.mkdirSync(lock);
      const workers = Array.from({ length: 6 }, () => contender(directory));
      try {
        await Promise.all(workers.map(worker => worker.ready));
        let initializationWatcher;
        if (mode === 'incomplete') {
          initializationWatcher = fs.watch(directory, (_, filename) => {
            if (filename !== 'state.lock.guard') return;
            assert.ok(fs.existsSync(lock), 'an initializing lock was reclaimed');
            fs.writeFileSync(path.join(lock, 'pid'), '99999999\n');
            fs.writeFileSync(path.join(lock, 'start_token'), 'dead-fixture\n');
            initializationWatcher.close();
          });
        }
        for (const worker of workers) worker.child.stdin.write('start\n');
        const outcomes = await Promise.all(workers.map(worker => worker.outcome));
        assert.equal(outcomes.filter(value => value === 'OWNER').length, 1);
        assert.ok(fs.readFileSync(path.join(lock, 'pid'), 'utf8').trim());
        assert.ok(fs.readFileSync(path.join(lock, 'start_token'), 'utf8').trim());
        assert.ok(fs.existsSync(path.join(lock, 'metadata.json')));
        workers[outcomes.indexOf('OWNER')].child.stdin.end('release\n');
        const results = await Promise.all(workers.map(worker => worker.exited));
        assert.equal(results.filter(result => result.code === 0).length, 1);
        assert.equal(results.filter(result => result.code === 75).length, 5);
        for (const result of results) {
          assert.doesNotMatch(result.errors, /No such file|ENOENT|not a child/);
          if (result.code === 75) assert.match(result.errors, /Timed out acquiring/);
        }
        assert.equal(fs.existsSync(lock), false);
      } finally {
        for (const worker of workers) worker.child.kill('SIGKILL');
        fs.rmSync(directory, { recursive: true, force: true });
      }
    }
  });
}

for (const abandoned of ['state.lock', 'state.lock.guard']) {
test(`abandoned ${abandoned} is refused without removing it`, { timeout: 5000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-incomplete-'));
  const lock = path.join(directory, abandoned);
  fs.mkdirSync(lock);
  fs.writeFileSync(path.join(lock, 'pid'), '99999999\n');
  const worker = contender(directory, 150);
  try {
    await worker.ready;
    worker.child.stdin.end('start\n');
    assert.equal(await worker.outcome, 'REFUSED');
    assert.equal((await worker.exited).code, 75);
    assert.equal(fs.readFileSync(path.join(lock, 'pid'), 'utf8'), '99999999\n');
  } finally { worker.child.kill('SIGKILL'); fs.rmSync(directory, { recursive: true, force: true }); }
});
}

test('live publication helper refuses a foreign caller; killed helper is reclaimed', { timeout: 10000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-publisher-'));
  const barrier = path.join(directory, 'barrier');
  const ready = path.join(directory, 'ready');
  assert.equal(spawnSync('mkfifo', [barrier]).status, 0);
  const watcher = fs.watch(directory);
  const published = new Promise(resolve => watcher.on('change', () => {
    if (fs.existsSync(ready)) resolve();
  }));
  const helper = spawn(process.execPath, [
    'scripts/build-gateway-mutex.mjs', 'acquire', directory, String(process.pid),
    nativeIdentity(process.pid).token, '5000', 'fixture',
  ], { cwd: root, env: { ...process.env, CT_GATEWAY_PUBLICATION_BARRIER: barrier, CT_GATEWAY_PUBLICATION_READY: ready }, stdio: 'ignore' });
  const helperExited = once(helper, 'exit');
  const workers = [];
  try {
    await published;
    watcher.close();
    const live = contender(directory, 150);
    workers.push(live);
    await live.ready;
    live.child.stdin.end('start\n');
    assert.equal(await live.outcome, 'REFUSED');
    const refused = await live.exited;
    assert.equal(refused.code, 75);
    assert.match(refused.errors, /rm -rf -- .*state.lock.guard/);
    assert.ok(fs.existsSync(path.join(directory, 'state.lock.guard')));
    helper.kill('SIGKILL');
    await helperExited;
    const recovered = contender(directory, 1500);
    workers.push(recovered);
    await recovered.ready;
    recovered.child.stdin.write('start\n');
    assert.equal(await recovered.outcome, 'OWNER');
    recovered.child.stdin.end('release\n');
    const result = await recovered.exited;
    assert.equal(result.code, 0);
    assert.doesNotMatch(result.errors, /ENOENT|No such file/);
    assert.equal(fs.existsSync(path.join(directory, 'state.lock.guard')), false);
  } finally {
    watcher.close();
    helper.kill('SIGKILL');
    for (const worker of workers) worker.child.kill('SIGKILL');
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

for (const recovery of ['reused-pid', 'retired-owner']) {
  test(`${recovery}: guard recovery preserves identity proof`, { timeout: 5000 }, async () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-guard-recovery-'));
    const guard = path.join(directory, 'state.lock.guard');
    fs.mkdirSync(guard);
    const inode = fs.statSync(guard);
    const owner = recovery === 'reused-pid'
      ? path.join(guard, 'owner.fixture')
      : path.join(directory, 'state.guard.retired.owner.fixture');
    fs.mkdirSync(owner);
    const pid = recovery === 'reused-pid' ? process.pid : 99999999;
    fs.writeFileSync(path.join(owner, 'pid'), `${pid}\n`);
    fs.writeFileSync(path.join(owner, 'start_token'), 'foreign-start-token\n');
    fs.writeFileSync(path.join(owner, 'guard_identity.json'), JSON.stringify({ pid, token: 'foreign-start-token', dev: inode.dev, ino: inode.ino }));
    const worker = contender(directory, 1500);
    try {
      await worker.ready;
      worker.child.stdin.write('start\n');
      assert.equal(await worker.outcome, 'OWNER');
      worker.child.stdin.end('release\n');
      assert.equal((await worker.exited).code, 0);
      assert.equal(fs.existsSync(guard), false);
    } finally { worker.child.kill('SIGKILL'); fs.rmSync(directory, { recursive: true, force: true }); }
  });
}
