// Caller: skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { nativeIdentity, publicationGuard } from './build-gateway-guard.mjs';
import { once } from 'node:events';
import { pathToFileURL } from 'node:url';
import readline from 'node:readline';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
function contender(directory, timeout = 20000, identityPreload = '') {
  const child = spawn(bash, ['-c', `
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
  `, 'fixture', directory, String(timeout)], { cwd: root, env: { ...process.env, ...(identityPreload ? { NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ''} --import=${pathToFileURL(identityPreload).href}` } : {}), PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH }, stdio: ['pipe', 'pipe', 'pipe'] });
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
async function cleanup(directory, workers) {
  for (const worker of workers) if (worker.child.exitCode === null) worker.child.kill('SIGKILL');
  await Promise.all(workers.map(worker => worker.exited));
  fs.rmSync(directory, { recursive: true, force: true });
}

function contentionIdentityFixture(directory) {
  const filename = path.join(directory, 'identity-fixture.mjs');
  fs.writeFileSync(filename, `
    import cp from 'node:child_process';
    import { syncBuiltinESMExports } from 'node:module';
    const original = cp.spawnSync;
    cp.spawnSync = function(command, args, ...rest) {
      const pid = command === 'powershell.exe' && args.join(' ').match(/GetProcessById\\((\\d+)\\)/)?.[1];
      if (!pid) return original.call(this, command, args, ...rest);
      let present = true;
      try { process.kill(Number(pid), 0); } catch { present = false; }
      return { status: 0, stdout: present ? 'fixture-' + pid : 'GONE', stderr: '' };
    };
    syncBuiltinESMExports();
  `);
  return filename;
}

for (const mode of ['fresh', 'incomplete']) {
  test(`${mode}: six concurrent Bash clients with fixture identities acquire and release in turn`, { timeout: 90000 }, async () => {
    for (let round = 0; round < 3; round++) {
      const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-contention-'));
      const lock = path.join(directory, 'state.lock');
      if (mode === 'incomplete') fs.mkdirSync(lock);
      const identityPreload = contentionIdentityFixture(directory);
      const workers = Array.from({ length: 6 }, () => contender(directory, 20000, identityPreload));
      let initializationWatcher;
      try {
        await Promise.all(workers.map(worker => worker.ready));
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
        const remaining = new Set(workers);
        while (remaining.size) {
          const { worker, outcome } = await Promise.race([...remaining].map(async worker => ({ worker, outcome: await worker.outcome })));
          assert.equal(outcome, 'OWNER', (outcome === 'REFUSED' ? (await worker.exited).errors : ''));
          assert.ok(fs.readFileSync(path.join(lock, 'pid'), 'utf8').trim());
          assert.ok(fs.readFileSync(path.join(lock, 'start_token'), 'utf8').trim());
          assert.ok(fs.existsSync(path.join(lock, 'metadata.json')));
          worker.child.stdin.end('release\n');
          const result = await worker.exited;
          assert.equal(result.code, 0, result.errors);
          assert.doesNotMatch(result.errors, /EPERM|ENOENT|Timed out|owner_lost/);
          remaining.delete(worker);
        }
        assert.equal(fs.existsSync(lock), false);
        assert.equal(fs.existsSync(path.join(directory, 'state.lock.guard')), false);
      } finally { initializationWatcher?.close(); await cleanup(directory, workers); }
    }
  });
}

for (const abandoned of ['state.lock', 'state.lock.guard']) {
  test(`incomplete ${abandoned} is refused without removing it`, { timeout: 15000 }, async () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-incomplete-'));
    const lock = path.join(directory, abandoned);
    if (abandoned === 'state.lock') {
      fs.mkdirSync(lock);
      fs.writeFileSync(path.join(lock, 'pid'), '99999999\n');
    } else fs.writeFileSync(lock, 'incomplete');
    const worker = contender(directory, 2000);
    try {
      await worker.ready;
      worker.child.stdin.end('start\n');
      assert.equal(await worker.outcome, 'REFUSED');
      assert.equal((await worker.exited).code, 75);
      assert.ok(fs.existsSync(lock));
    } finally { await cleanup(directory, [worker]); }
  });
}

test('live publication helper refuses a foreign caller; killed helper is reclaimed', { timeout: 30000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-publisher-'));
  const ready = path.join(directory, 'ready');
  const watcher = fs.watch(directory);
  const published = new Promise(resolve => watcher.on('change', () => {
    if (fs.existsSync(ready)) resolve();
  }));
  const helper = spawn(process.execPath, [
    'scripts/build-gateway-mutex.mjs', 'acquire', directory, String(process.pid),
    nativeIdentity(process.pid).token, '20000', 'fixture',
  ], { cwd: root, env: { ...process.env, CT_GATEWAY_PUBLICATION_BARRIER: 'stdin', CT_GATEWAY_PUBLICATION_READY: ready }, stdio: ['pipe', 'ignore', 'pipe'] });
  const helperExited = once(helper, 'exit');
  const workers = [];
  try {
    await published;
    watcher.close();
    const live = contender(directory, 2000);
    workers.push(live);
    await live.ready;
    live.child.stdin.end('start\n');
    assert.equal(await live.outcome, 'REFUSED');
    assert.equal((await live.exited).code, 75);
    helper.kill('SIGKILL');
    await helperExited;
    const recovered = contender(directory);
    workers.push(recovered);
    await recovered.ready;
    recovered.child.stdin.write('start\n');
    assert.equal(await recovered.outcome, 'OWNER');
    recovered.child.stdin.end('release\n');
    const result = await recovered.exited;
    assert.equal(result.code, 0, result.errors);
    assert.equal(fs.existsSync(path.join(directory, 'state.lock.guard')), false);
  } finally {
    watcher.close();
    if (helper.exitCode === null) helper.kill('SIGKILL');
    await helperExited;
    await cleanup(directory, workers);
  }
});

test('reused native pid does not keep a dead guard owner alive', { timeout: 15000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-guard-recovery-'));
  const guard = path.join(directory, 'state.lock.guard');
  fs.writeFileSync(guard, JSON.stringify({ pid: process.pid, token: 'foreign-start-token', owner: 'fixture' }));
  const worker = contender(directory);
  try {
    await worker.ready;
    worker.child.stdin.write('start\n');
    assert.equal(await worker.outcome, 'OWNER');
    worker.child.stdin.end('release\n');
    assert.equal((await worker.exited).code, 0);
    assert.equal(fs.existsSync(guard), false);
  } finally { await cleanup(directory, [worker]); }
});


test('transient publication helper identity waits for a filesystem event before publishing', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-helper-identity-'));
  let available = false;
  const publisher = publicationGuard(directory, () => available ? { state: 'present', token: 'fixture' } : { state: 'unknown' });
  const watcher = fs.watch(directory);
  try {
    assert.equal(publisher.acquire(), false);
    assert.equal(publisher.identityUnavailable, true);
    assert.deepEqual(fs.readdirSync(directory), []);
    const event = once(watcher, 'change');
    available = true;
    fs.writeFileSync(path.join(directory, 'lookup-recovered'), 'ready');
    await event;
    assert.equal(publisher.acquire(), true);
    assert.equal(publisher.identityUnavailable, false);
    publisher.release();
    assert.equal(fs.existsSync(path.join(directory, 'state.lock.guard')), false);
  } finally { watcher.close(); fs.rmSync(directory, { recursive: true, force: true }); }
});

for (const mode of ['recovers', 'refuses']) {
test(`transient helper lookup ${mode} through the real mutex filesystem wait`, { timeout: 15000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-helper-event-'));
  const recovered = path.join(directory, 'lookup-recovered');
  const preload = path.join(directory, 'lookup-fixture.mjs');
  fs.writeFileSync(preload, `
    import fs from 'node:fs';
    import cp from 'node:child_process';
    import { syncBuiltinESMExports } from 'node:module';
    const recovered = ${JSON.stringify(recovered)};
    let announced = false;
    function unavailable() {
      if (fs.existsSync(recovered)) return false;
      if (!announced) { process.stdout.write('LOOKUP_UNKNOWN\\n'); announced = true; }
      return true;
    }
    const read = fs.readFileSync;
    fs.readFileSync = function(file, ...args) {
      if (String(file) === '/proc/' + process.pid + '/stat' && unavailable()) {
        throw Object.assign(new Error('fixture unavailable'), { code: 'EACCES' });
      }
      return read.call(this, file, ...args);
    };
    const spawn = cp.spawnSync;
    cp.spawnSync = function(command, args, ...rest) {
      const self = command === 'powershell.exe' && args.some(value => value.includes('GetProcessById(' + process.pid + ')')) ||
        command === 'ps' && args.includes(String(process.pid));
      if (self && unavailable()) return { status: 2, stdout: '', stderr: '' };
      return spawn.call(this, command, args, ...rest);
    };
    syncBuiltinESMExports();
  `);
  const child = spawn(process.execPath, ['--import', pathToFileURL(preload).href,
    path.join(root, 'scripts/build-gateway-mutex.mjs'), 'acquire', directory, '99999999', 'fixture', mode === 'refuses' ? '1000' : '10000', 'fixture'],
  { cwd: root, env: process.env, stdio: ['ignore', 'pipe', 'pipe'] });
  const lines = readline.createInterface({ input: child.stdout });
  let errors = '';
  child.stderr.on('data', chunk => errors += chunk);
  const exited = once(child, 'exit');
  try {
    const [line] = await once(lines, 'line');
    assert.equal(line, 'LOOKUP_UNKNOWN');
    assert.equal(fs.existsSync(path.join(directory, 'state.lock.guard')), false);
    if (mode === 'recovers') {
      fs.writeFileSync(recovered, 'ready');
      assert.equal((await exited)[0], 0, errors);
      assert.equal(fs.readFileSync(path.join(directory, 'state.lock/pid'), 'utf8').trim(), '99999999');
    } else {
      assert.equal((await exited)[0], 75, errors);
      assert.match(errors, /identity unavailable; no ownership published\. Retry-safe/);
      assert.equal(fs.existsSync(path.join(directory, 'state.lock')), false);
    }
    assert.doesNotMatch(errors, /Cannot establish|Error:/);
  } finally {
    lines.close();
    if (child.exitCode === null) child.kill('SIGKILL');
    await exited;
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

}

test('a blocked owner probe leaves publication free and cannot reclaim a replacement owner', { timeout: 30000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'mutex-probe-barrier-'));
  const mutex = path.join(directory, 'state.lock');
  const publishOwner = (pid, token) => {
    fs.mkdirSync(mutex, { recursive: true });
    fs.writeFileSync(path.join(mutex, 'pid'), pid);
    fs.writeFileSync(path.join(mutex, 'start_token'), token);
  };
  publishOwner('101', 'old-token');
  const preload = path.join(directory, 'probe-barrier.mjs');
  fs.writeFileSync(preload, `
    import fs from 'node:fs';
    import cp from 'node:child_process';
    import { syncBuiltinESMExports } from 'node:module';
    const original = cp.spawnSync;
    cp.spawnSync = function(command, args, ...rest) {
      if (command === 'powershell.exe' && args.join(' ').includes('GetProcessById')) return { status: 0, stdout: 'fixture-native-token' };
      if (args?.[1]?.includes('ct_gateway_identity_alive')) {
        if (args.at(-2) === '303') { process.stdout.write('SUCCESSOR_ALIVE\\n'); return { status: 0 }; }
        return { status: 1 };
      }
      if (args?.[1]?.includes('ct_gateway_owner_gone') && args.at(-2) === '101') {
        process.stdout.write('PROBE_BLOCKED\\n');
        fs.readSync(0, Buffer.alloc(1), 0, 1, null);
        return { status: 0 };
      }
      return original.call(this, command, args, ...rest);
    };
    syncBuiltinESMExports();
  `);
  const helper = path.join(root, 'scripts/build-gateway-mutex.mjs');
  const argumentsFor = (operation, pid, token) => ['--import', pathToFileURL(preload).href, helper, operation, directory, pid, token, '20000', 'fixture'];
  const child = spawn(process.execPath, argumentsFor('acquire', '202', 'new-token'), { stdio: ['pipe', 'pipe', 'pipe'] });
  const lines = readline.createInterface({ input: child.stdout });
  const exited = once(child, 'exit');
  let errors = '';
  child.stderr.on('data', chunk => { errors += chunk; });
  const release = async (pid, token) => {
    const releaser = spawn(process.execPath, argumentsFor('release', pid, token), { stdio: ['ignore', 'ignore', 'pipe'] });
    let detail = '';
    releaser.stderr.on('data', chunk => { detail += chunk; });
    assert.equal((await once(releaser, 'exit'))[0], 0, detail);
  };
  try {
    assert.equal((await once(lines, 'line'))[0], 'PROBE_BLOCKED');
    assert.equal(fs.existsSync(path.join(directory, 'state.lock.guard')), false);
    await release('101', 'old-token');
    publishOwner('303', 'successor-token');
    const successorObserved = once(lines, 'line');
    child.stdin.write('continue\n');
    assert.equal((await successorObserved)[0], 'SUCCESSOR_ALIVE');
    assert.equal(fs.readFileSync(path.join(mutex, 'pid'), 'utf8'), '303');
    await release('303', 'successor-token');
    assert.equal((await exited)[0], 0, errors);
    assert.equal(fs.readFileSync(path.join(mutex, 'pid'), 'utf8').trim(), '202');
  } finally {
    lines.close();
    if (child.exitCode === null) child.kill('SIGKILL');
    await exited;
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

for (const mode of ['recovers', 'refuses']) {
test(`Windows publication EPERM ${mode} through ownership events without leaking pending directories`, { timeout: 20000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-publish-sharing-'));
  const mutex = path.join(directory, 'state.lock');
  const preload = path.join(directory, 'publication-sharing.mjs');
  const attempts = path.join(directory, 'rename-attempts');
  fs.writeFileSync(preload, `
    import fs from 'node:fs';
    import path from 'node:path';
    import cp from 'node:child_process';
    import { syncBuiltinESMExports } from 'node:module';
    const rename = fs.renameSync;
    let attempts = 0;
    fs.renameSync = function(from, to) {
      if (process.env.PUBLICATION_SHARING_MODE && path.basename(from).startsWith('state.lock.pending.') && path.basename(to) === 'state.lock') {
        attempts++;
        fs.writeFileSync(${JSON.stringify(attempts)}, String(attempts));
        if (process.env.PUBLICATION_SHARING_MODE === 'always' || attempts === 1) {
          queueMicrotask(() => process.stdout.write('PUBLICATION_WAITING\\n'));
          throw Object.assign(new Error('simulated Windows sharing violation'), { code: 'EPERM' });
        }
      }
      return rename.call(this, from, to);
    };
    const spawn = cp.spawnSync;
    cp.spawnSync = function(command, args, ...rest) {
      if (command === 'powershell.exe' && args.join(' ').includes('GetProcessById')) return { status: 0, stdout: 'fixture-native-token' };
      if (args?.[1]?.includes('ct_gateway_identity_alive')) {
        if (args.at(-2) === '303') { process.stdout.write('FOREIGN_OWNER\\n'); return { status: 0 }; }
        return { status: 1 };
      }
      return spawn.call(this, command, args, ...rest);
    };
    syncBuiltinESMExports();
  `);
  const helper = path.join(root, 'scripts/build-gateway-mutex.mjs');
  const workers = [];
  const start = (operation, pid, token, fault = '') => {
    const child = spawn(process.execPath, ['--import', pathToFileURL(preload).href, helper, operation, directory, pid, token, fault === 'always' ? '1000' : '15000', 'fixture'], {
      stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, PUBLICATION_SHARING_MODE: fault },
    });
    const lines = readline.createInterface({ input: child.stdout });
    let errors = '';
    child.stderr.on('data', chunk => { errors += chunk; });
    const exited = once(child, 'close').then(([code]) => ({ code, errors }));
    const worker = { child, lines, exited };
    workers.push(worker);
    return worker;
  };
  const pendingDirectories = () => fs.readdirSync(directory).filter(name => name.startsWith('state.lock.pending.'));
  const blocked = start('acquire', '202', 'new-token', mode === 'recovers' ? 'once' : 'always');
  try {
    assert.equal((await once(blocked.lines, 'line'))[0], 'PUBLICATION_WAITING');
    assert.equal(fs.existsSync(path.join(directory, 'state.lock.guard')), false, 'sharing violation releases publication exclusivity');
    assert.equal(pendingDirectories().length, 1);
    fs.writeFileSync(path.join(directory, 'unrelated-event'), 'not ownership readiness');
    if (mode === 'recovers') {
      const observedOwner = once(blocked.lines, 'line');
      const foreign = start('acquire', '303', 'foreign-token');
      assert.equal((await foreign.exited).code, 0);
      assert.equal((await observedOwner)[0], 'FOREIGN_OWNER');
      assert.equal(fs.readFileSync(path.join(mutex, 'pid'), 'utf8').trim(), '303');
      assert.equal(fs.readFileSync(attempts, 'utf8'), '1', 'self and unrelated events do not retry publication');
      const released = start('release', '303', 'foreign-token');
      assert.equal((await released.exited).code, 0);
      const result = await blocked.exited;
      assert.equal(result.code, 0, result.errors);
      assert.equal(fs.readFileSync(attempts, 'utf8'), '2');
      assert.equal(fs.readFileSync(path.join(mutex, 'pid'), 'utf8').trim(), '202');
      assert.equal((await start('release', '202', 'new-token').exited).code, 0);
    } else {
      const result = await blocked.exited;
      assert.equal(result.code, 75, result.errors);
      assert.match(result.errors, /Timed out acquiring build gateway state mutex/);
      assert.doesNotMatch(result.errors, /simulated Windows sharing violation|EPERM: operation/);
      assert.equal(fs.readFileSync(attempts, 'utf8'), '1');
    }
    assert.deepEqual(pendingDirectories(), []);
    assert.equal(fs.existsSync(path.join(directory, 'state.lock.guard')), false);
    assert.equal(fs.existsSync(mutex), false);
  } finally {
    for (const worker of workers) {
      worker.lines.close();
      if (worker.child.exitCode === null) worker.child.kill('SIGKILL');
    }
    await Promise.all(workers.map(worker => worker.exited));
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
}
