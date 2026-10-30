import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { execFile } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { adoptLegacyTarget, refusal, runningFromTarget, stopCommand } from '../dev-named-target.mjs';
import { acquireLaunchOwner, releaseTargetReservation, validateTargetReservation } from '../dev-launch-owner.mjs';

function fixture() {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'dev-shared-target-')));
  const checkout = path.join(root, 'checkout'), home = path.join(root, 'home');
  fs.mkdirSync(path.join(checkout, 'src-tauri/target'), { recursive: true });
  fs.mkdirSync(home);
  const legacy = name => {
    const directory = path.join(checkout, 'src-tauri/target/dev-instances', name);
    fs.mkdirSync(path.join(directory, 'debug/incremental'), { recursive: true });
    fs.writeFileSync(path.join(directory, 'debug/marker'), name);
    return directory;
  };
  const instance = (name, port, target, built) => {
    const data = path.join(home, `.domios-dev-${name}`);
    fs.mkdirSync(data, { recursive: true });
    const file = path.join(data, 'dev-build-provenance.json');
    fs.writeFileSync(file, JSON.stringify({ schema: 3, instance: { name, apiPort: String(port), identifier: `com.domios.dev.${name}` },
      artifacts: { server: { path: path.join(target, 'debug/domios-server') } } }));
    if (built) fs.utimesSync(file, built, built);
    return data;
  };
  return { root, checkout, home, legacy, instance, shared: path.join(checkout, 'src-tauri/target/named-dev') };
}

const inspect = () => ({ processes: [], listeners: [] });

const health = live => async url => {
  const port = Number(new URL(url).port);
  const match = live.find(row => row.port === port);
  if (!match) throw Error('connection refused');
  return { ok: true, json: async () => ({ instance: { name: match.name, api_port: String(port), identifier: `com.domios.dev.${match.name}` } }) };
};

for (const failure of ['timeout', 'non-OK']) for (const owner of ['unreadable', 'missing', 'identified']) {
  test(`server-only listener with ${owner} identity refuses mutation with ${failure} health`, async t => {
    const { root, checkout, home, legacy, instance, shared } = fixture();
    t.after(() => fs.rmSync(root, { recursive: true, force: true }));
    const target = legacy('server-only');
    instance('server-only', 17823, target);
    const row = owner === 'identified' ? { pid: 42, created: '123', executable: path.join(target, 'debug/domios-server') }
      : { pid: 42, created: '', executable: '' };
    let renames = 0, buildAdmissions = 0;
    const options = { inspect: () => ({ processes: owner === 'missing' ? [] : [row], listeners: [{ port: 17823, pid: 42 }] }),
      request: async () => {
        if (failure === 'timeout') throw Error('timeout');
        return { ok: false };
      }, rename: () => { renames++; }, log: () => {},
    };
    const blocking = await runningFromTarget(target, home, options);
    assert.equal(blocking[0].state, owner === 'identified' ? 'running' : 'unknown');
    if (!refusal(target, 'rebuild', blocking)) buildAdmissions++;
    await assert.rejects(adoptLegacyTarget(checkout, home, options), /refusing adoption/);
    assert.equal(renames, 0);
    assert.equal(buildAdmissions, 0);
    assert.equal(fs.existsSync(target), true);
    assert.equal(fs.existsSync(shared), false);
    const gone = { ...options, inspect: () => ({ processes: [], listeners: [] }) };
    assert.deepEqual(await runningFromTarget(target, home, gone), []);
    assert.equal(refusal(target, 'rebuild', await runningFromTarget(target, home, gone)), null);
    await adoptLegacyTarget(checkout, home, gone);
    assert.equal(renames, 1);
  });
}

for (const failure of ['unhealthy', 'timeout', 'mismatch', 'client-only']) {
  test(`live process refuses adoption and rebuild with ${failure} health`, async t => {
    const { root, checkout, home, legacy, instance, shared } = fixture();
    t.after(() => fs.rmSync(root, { recursive: true, force: true }));
    const target = legacy('alpha');
    const data = instance('alpha', 17821, target);
    const process = { pid: 42, created: '123', executable: path.join(target, failure === 'client-only' ? 'client' : 'server') };
    const file = path.join(data, 'dev-build-provenance.json');
    const record = JSON.parse(fs.readFileSync(file, 'utf8'));
    record.nativeClient = { snapshot: { client_process: process } };
    fs.writeFileSync(file, JSON.stringify(record));
    const options = {
      inspect: () => ({ processes: [process], listeners: [] }),
      request: async () => {
        if (failure === 'timeout' || failure === 'client-only') throw Error('timeout');
        return { ok: failure !== 'unhealthy', json: async () => ({ instance: { name: 'beta' } }) };
      }, log: () => {},
    };
    const blocking = await runningFromTarget(target, home, options);
    assert.equal(blocking.length, 1);
    assert.match(refusal(target, 'rebuild', blocking), /refusing rebuild/);
    await assert.rejects(adoptLegacyTarget(checkout, home, options), /refusing adoption/);
    assert.equal(fs.existsSync(target), true);
    assert.equal(fs.existsSync(shared), false);
    delete record.nativeClient;
    fs.writeFileSync(file, JSON.stringify(record));
    const uncertain = await runningFromTarget(target, home, options);
    assert.equal(uncertain[0].state, 'unknown');
    await assert.rejects(adoptLegacyTarget(checkout, home, options), /refusing adoption/);
  });
}

test('unrecorded target survivor and failed inspection both refuse mutation', async t => {
  const { root, home, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const inspect of [() => ({ processes: [{ pid: 42, executable: path.join(shared, 'client') }], listeners: [] }),
    () => { throw Error('inspection unavailable'); }]) {
    const blocking = await runningFromTarget(shared, home, { inspect });
    assert.equal(blocking.length, 1);
    assert.match(refusal(shared, 'rebuild', blocking), /refusing rebuild/);
  }
});

test('retained process identity must be inspected before health failure can mean stopped', async t => {
  const { root, home, instance, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const data = instance('alpha', 17822, shared);
  const file = path.join(data, 'dev-build-provenance.json');
  const record = JSON.parse(fs.readFileSync(file, 'utf8'));
  record.nativeClient = { snapshot: { client_process: { pid: 42, created: '123', executable: path.join(shared, 'client') } } };
  fs.writeFileSync(file, JSON.stringify(record));
  const options = { request: async () => { throw Error('timeout'); } };
  const unknown = await runningFromTarget(shared, home, { ...options, inspect: () => ({ processes: [{ pid: 42 }], listeners: [] }) });
  assert.equal(unknown[0].state, 'unknown');
  assert.deepEqual(await runningFromTarget(shared, home, { ...options, inspect }), []);
});

test('target reservation excludes a launch during scan/mutation and releases on native readiness', async t => {
  const { root } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const reservation = await acquireLaunchOwner(root, { releaseOnReady: true });
  const environment = { CODETERM_DEV_TARGET_CHILD: '1',
    CODETERM_DEV_TARGET_SOCKET: reservation.environment.CODETERM_DEV_LAUNCH_SOCKET,
    CODETERM_DEV_TARGET_TOKEN: reservation.environment.CODETERM_DEV_LAUNCH_TOKEN };
  await assert.rejects(validateTargetReservation(root, { CODETERM_DEV_TARGET_CHILD: '1' }), /no matching live lease/);
  await assert.rejects(validateTargetReservation(root, { ...environment, CODETERM_DEV_TARGET_TOKEN: 'stale-token' }), /disconnected/);
  await assert.rejects(validateTargetReservation(path.join(root, 'checkout'), environment), /no matching live lease/);
  await validateTargetReservation(root, environment);
  await assert.rejects(acquireLaunchOwner(root), /unavailable/);
  await releaseTargetReservation({ CODETERM_DEV_TARGET_SOCKET: reservation.environment.CODETERM_DEV_LAUNCH_SOCKET,
    CODETERM_DEV_TARGET_TOKEN: reservation.environment.CODETERM_DEV_LAUNCH_TOKEN });
  await reservation.finish();
  await assert.rejects(validateTargetReservation(root, environment));
  const next = await acquireLaunchOwner(root);
  await next.finish();
});

test('adoption renames the single legacy target once and never copies or deletes', async t => {
  const { root, checkout, home, legacy, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = legacy('qa8');
  const logs = [];
  const adopted = await adoptLegacyTarget(checkout, home, { inspect, log: line => logs.push(line), request: health([]) });
  assert.deepEqual(adopted, { from: source, to: shared });
  assert.equal(fs.readFileSync(path.join(shared, 'debug/marker'), 'utf8'), 'qa8');
  assert.equal(fs.existsSync(source), false);
  assert.match(logs.join('\n').replaceAll('\\', '/'), /adopted .*dev-instances\/qa8 as .*named-dev/);
  legacy('later');
  assert.equal(await adoptLegacyTarget(checkout, home, { inspect, log: () => {}, request: health([]) }), null);
  assert.equal(fs.readFileSync(path.join(checkout, 'src-tauri/target/dev-instances/later/debug/marker'), 'utf8'), 'later');
});

test('adoption among several legacy targets takes the newest provenance and leaves the others', async t => {
  const { root, checkout, home, legacy, instance, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const older = legacy('older'), newer = legacy('newer'), orphan = legacy('orphan');
  instance('older', 17801, older, new Date('2026-10-01T00:00:00Z'));
  instance('newer', 17802, newer, new Date('2026-10-20T00:00:00Z'));
  await adoptLegacyTarget(checkout, home, { inspect, log: () => {}, request: health([]) });
  assert.equal(fs.readFileSync(path.join(shared, 'debug/marker'), 'utf8'), 'newer');
  for (const directory of [older, orphan]) assert.equal(fs.existsSync(directory), true);
});

test('adoption refuses without moving while a DEV still runs from the selected legacy target', async t => {
  const { root, checkout, home, legacy, instance, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const source = legacy('qa8');
  instance('qa8', 17803, source);
  await assert.rejects(adoptLegacyTarget(checkout, home, { inspect, log: () => {}, request: health([{ name: 'qa8', port: 17803 }]) }),
    /qa8 \(api 17803.*\n.*\n.*CODETERM_DEV_INSTANCE='qa8' CODETERM_DEV_API_PORT='17803' \.\/build\.sh kill/s);
  assert.equal(fs.existsSync(source), true);
  assert.equal(fs.existsSync(shared), false);
});

test('several legacy targets without provenance are left alone', async t => {
  const { root, checkout, home, legacy, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  legacy('one'); legacy('two');
  const logs = [];
  assert.equal(await adoptLegacyTarget(checkout, home, { inspect, log: line => logs.push(line), request: health([]) }), null);
  assert.equal(fs.existsSync(shared), false);
  assert.match(logs[0], /not adopting: 2 targets/);
});

test('running DEVs are those whose own provenance names the target and whose port answers as that instance', async t => {
  const { root, home, instance, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const elsewhere = path.join(root, 'other-checkout/src-tauri/target/named-dev');
  const alpha = instance('alpha', 17811, shared);
  instance('beta', 17812, shared);
  instance('gamma', 17813, shared);
  instance('foreign', 17814, elsewhere);
  const live = [{ name: 'alpha', port: 17811 }, { name: 'squatter', port: 17812 }, { name: 'foreign', port: 17814 }];
  const running = await runningFromTarget(shared, home, { inspect, request: health(live) });
  assert.deepEqual(running.map(row => [row.name, row.port, row.data]), [['alpha', 17811, alpha]]);
  const message = refusal(shared, 'rebuild (dev)', running);
  assert.match(message, /refusing rebuild \(dev\)/);
  assert.match(message, new RegExp(`alpha \\(api 17811, data ${alpha.replaceAll('\\', '\\\\')}\\)`));
  assert.ok(message.includes(stopCommand(running[0])));
  assert.equal(refusal(shared, 'rebuild (dev)', []), null);
});

test('with both instances running, dev for either names the other, its kill command and the selected instance', async t => {
  const { root, home, instance, shared } = fixture();
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const servers = await Promise.all(['alpha', 'beta'].map(name => new Promise(resolve => {
    const server = http.createServer((request, response) => {
      response.setHeader('content-type', 'application/json');
      response.end(JSON.stringify({ instance: { name, api_port: String(server.address().port), identifier: `com.domios.dev.${name}` } }));
    });
    server.listen(0, '127.0.0.1', () => resolve(server));
  })));
  t.after(() => servers.forEach(server => server.close()));
  const [alphaPort, betaPort] = servers.map(server => server.address().port);
  instance('alpha', alphaPort, shared);
  instance('beta', betaPort, shared);
  const script = fileURLToPath(new URL('../dev-named-target.mjs', import.meta.url));
  for (const [self, other, otherPort, selfPort] of [['alpha', 'beta', betaPort, alphaPort], ['beta', 'alpha', alphaPort, betaPort]]) {
    const result = await new Promise(resolve => execFile(process.execPath, [script, 'refuse-running', shared, home, self, 'rebuild (dev)'],
      (error, stdout, stderr) => resolve({ status: error?.code ?? 0, stderr })));
    assert.equal(result.status, 2, result.stderr);
    assert.match(result.stderr, new RegExp(`- ${other} \\(api ${otherPort},`));
    assert.ok(result.stderr.includes(`CODETERM_DEV_INSTANCE='${other}' CODETERM_DEV_API_PORT='${otherPort}' ./build.sh kill`));
    assert.match(result.stderr, new RegExp(`selected instance ${self} \\(api ${selfPort}\\) is also running`));
    assert.ok(result.stderr.includes(`CODETERM_DEV_INSTANCE='${self}' CODETERM_DEV_API_PORT='${selfPort}' ./build.sh kill`));
  }
  servers[1].close();
  const alone = await new Promise(resolve => execFile(process.execPath, [script, 'refuse-running', shared, home, 'alpha', 'rebuild (dev)'],
    (error, stdout, stderr) => resolve({ status: error?.code ?? 0, stderr })));
  assert.equal(alone.status, 2);
  assert.match(alone.stderr, /selected instance alpha/);
});
