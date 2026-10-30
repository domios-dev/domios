import test from 'node:test';
import assert from 'node:assert/strict';
import { instanceTreePlan, namedInstancePlan, namedInstanceRoot, verifiedQuitProcesses, gracefulQuit, stopInstanceTree } from '../dev-instance-processes.mjs';
import { mkdtempSync, mkdirSync, writeFileSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

for (const windows of [false, true]) {
  const prefix = windows ? 'C:/repo' : '/repo';
  const root = `${prefix}/src-tauri/target/dev-instances/own`;
  const row = (pid, executable, parent = 1) => ({ pid, executable, parent, created: `${pid}` });
  test(`instance tree selects every own image, including detached orphans (${windows ? 'Windows' : 'Unix'})`, () => {
    const processes = [
      row(1, `${root}/debug/domios-dev-app.exe`),
      row(2, `${root}/debug/domios-server/domios-server.exe`),
      row(3, `${root}/debug/conpty/conpty-agent.exe`, 2),
      row(4, `${root}/debug/domios-server/domios-server.exe`, 999),
      row(5, `${prefix}/src-tauri/target/dev-instances/foreign/debug/domios-dev-app.exe`, 1),
      row(6, `${prefix}/src-tauri/target/debug/domios-dev-app.exe`, 1),
      row(7, `${prefix}/AppData/Local/Domios/domios-server.exe`, 1),
      row(8, `${root}-foreign/debug/domios-dev-app.exe`),
      row(9, `${root}/../foreign/debug/domios-server.exe`),
      row(10, `${prefix}/system/shell`, 2),
    ];
    const result = instanceTreePlan({ root, windows, port: 17996, processes, listeners: [{ port: 7685, pid: 7 }] });
    assert.deepEqual(result, processes.slice(0, 4));
    assert.deepEqual(instanceTreePlan({ root, windows, port: 17996, processes: processes.slice(4), listeners: [] }), []);
  });

  test(`production ownership and unknown creation identity refuse (${windows ? 'Windows' : 'Unix'})`, () => {
    const processes = [row(42, `${root}/debug/server`)];
    assert.throws(() => instanceTreePlan({ root, windows, port: 7685, processes: [], listeners: [] }), /production/);
    assert.throws(() => instanceTreePlan({ root, windows, port: 17996, processes, listeners: [{ port: 7685, pid: 42 }] }), /production/);
    assert.throws(() => instanceTreePlan({ root, windows, port: 17996, processes: [{ ...processes[0], created: '' }], listeners: [] }), /creation identity/);
  });
}

test('Windows image spelling normalizes separators and extended paths; Unix remains case sensitive', () => {
  const root = 'C:/repo/target/dev-instances/own';
  const process = { pid: 42, created: '123', executable: '\\\\?\\C:\\REPO\\target\\dev-instances\\own\\debug\\server.exe' };
  assert.deepEqual(instanceTreePlan({ root, windows: true, port: 17996, processes: [process], listeners: [] }), [process]);
  assert.deepEqual(instanceTreePlan({ root: '/repo/own', windows: false, port: 17996,
    processes: [{ ...process, executable: '/repo/Own/server' }], listeners: [] }), []);
});

test('canonical DEV selection excludes named instance trees and installed production', () => {
  const processes = [
    { pid: 1, created: '1', executable: '/repo/src-tauri/target/debug/domios-dev-app' },
    { pid: 2, created: '2', executable: '/repo/src-tauri/target/dev-instances/own/debug/server' },
    { pid: 3, created: '3', executable: '/opt/Domios/domios-server' },
  ];
  assert.deepEqual(instanceTreePlan({ root: '/repo/src-tauri/target/debug', windows: false, port: 7686,
    processes, listeners: [{ port: 7685, pid: 3 }] }), [processes[0]]);
});

for (const windows of [true, false]) test(`canonical cleanup excludes concurrent Cargo images (${windows ? 'Windows' : 'Unix'})`, () => {
  const root = `${windows ? 'C:' : ''}/repo/src-tauri/target/triple/debug`;
  const suffix = windows ? '.exe' : '';
  const processes = ['domios-dev-app', 'domios-server/domios-server', 'cli/codeterm',
    'deps/domios_server-abc', 'build/dependency-abc/build-script-build', 'examples/demo',
    'incremental/fixture', 'deps/domios-dev-app', 'other/domios-server'].map((name, index) => ({
    pid: index + 1, created: String(index + 100), executable: `${root}/${name}${suffix}`,
  }));
  assert.deepEqual(instanceTreePlan({ root, windows, port: 7686, processes, listeners: [] }), processes.slice(0, 3));
});

test('graceful quit accepts only selected incarnations and the exact DEV listener/data identity', () => {
  const server = { pid: 42, created: '123', executable: '/repo/target/debug/domios-server' };
  const input = { port: 7686, data: '/data', processes: [server], listeners: [{ pid: 42, port: 7686 }], windows: false };
  const quit = { ready: { process: server, api_address: '127.0.0.1:7686', identity: { data: '/data', config: '/data/config.toml' } } };
  assert.deepEqual(verifiedQuitProcesses(quit, input), [42]);
  assert.deepEqual(verifiedQuitProcesses({ ready: { ...quit.ready, process: { ...server, created: '999' } } }, input), []);
  assert.deepEqual(verifiedQuitProcesses({ ready: { ...quit.ready, identity: { data: '/foreign' } } }, input), []);
  assert.deepEqual(verifiedQuitProcesses(quit, { ...input, listeners: [{ pid: 99, port: 7686 }] }), []);
  assert.deepEqual(verifiedQuitProcesses({ ...quit, client: { process: { ...server, pid: 99 } } }, input), []);
});

test('clean shutdown requests the authenticated quit snapshot before posting that exact snapshot', async () => {
  const directory = mkdtempSync(path.join(tmpdir(), 'domios-clean-quit-'));
  try {
    const data = realpathSync.native(directory);
    mkdirSync(path.join(data, 'bin'));
    writeFileSync(path.join(data, 'bin', `codeterm${process.platform === 'win32' ? '.exe' : ''}`), 'fixture');
    const server = { pid: 42, created: '123', executable: path.join(data, 'server') };
    const quit = { ready: { process: server, api_address: '127.0.0.1:17996', identity: { data, config: path.join(data, 'config.toml') } } };
    const calls = [];
    const ids = await gracefulQuit({ processes: [server], listeners: [{ pid: 42, port: 17996 }], data, port: 17996 }, {
      run: () => { calls.push('token'); return { status: 0, stdout: 'opaque-fixture-token' }; },
      request: async (url, options) => {
        assert.equal(options.headers.Authorization, 'Bearer opaque-fixture-token');
        if (url.endsWith('/snapshot')) { calls.push('snapshot'); return { ok: true, json: async () => quit }; }
        calls.push('quit');
        assert.equal(options.method, 'POST');
        assert.deepEqual(JSON.parse(options.body), quit);
        return { ok: true, json: async () => ({ outcome: 'completed', expected: quit,
          shutdown_completion: { operation: 'shutdown', cleanup: 'completed' } }) };
      },
    });
    assert.deepEqual(calls, ['token', 'snapshot', 'quit']);
    assert.deepEqual(ids, [42]);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

for (const windows of [false, true]) test(`named kill requires authenticated seeds even with an API listener in the shared target (${windows ? 'Windows' : 'Unix'})`, () => {
  const prefix = windows ? 'C:/repo' : '/repo';
  const root = `${prefix}/src-tauri/target/named-dev`;
  const row = (pid, executable, parent) => ({ pid, parent, created: `${pid}`, executable: `${root}/debug/${executable}` });
  const processes = [
    row(10, 'domios-dev-app', 1), row(11, 'domios-server/domios-server', 10), row(12, 'conpty/x64/OpenConsole.exe', 11),
    row(20, 'domios-dev-app', 1), row(21, 'domios-server/domios-server', 20),
    row(30, 'domios-dev-app', 1),
    { pid: 40, parent: 11, created: '40', executable: `${prefix}/bin/shell` },
  ];
  const listeners = [{ port: 17996, pid: 11 }, { port: 17997, pid: 21 }, { port: 7685, pid: 99 }];
  assert.deepEqual(namedInstancePlan({ root, windows, port: 17996, processes, listeners }), []);
  const plan = namedInstancePlan({ root, windows, port: 17996, processes, listeners, seeds: [10, 11] });
  assert.deepEqual(plan.map(row => row.pid), [10, 11, 12]);
  assert.deepEqual(namedInstancePlan({ root, windows, port: 17996, processes, listeners, seeds: [30] }).map(row => row.pid), [30]);
  assert.deepEqual(namedInstancePlan({ root, windows, port: 17998, processes, listeners }), []);
  assert.throws(() => namedInstancePlan({ root, windows, port: 7685, processes, listeners }), /production/);
});

test('named kill resolves a legacy target only when the instance record names it', () => {
  const directory = realpathSync.native(mkdtempSync(path.join(tmpdir(), 'named-instance-root-')));
  try {
    const legacy = path.join(directory, 'src-tauri/target/dev-instances/qa8');
    const data = path.join(directory, 'home/.domios-dev-qa8');
    mkdirSync(legacy, { recursive: true });
    mkdirSync(data, { recursive: true });
    const record = server => writeFileSync(path.join(data, 'dev-build-provenance.json'), JSON.stringify({ schema: 3, instance: { name: 'qa8' }, artifacts: { server: { path: server } } }));
    record(path.join(legacy, 'debug/domios-server'));
    assert.equal(namedInstanceRoot(directory, data), legacy);
    record(path.join(directory, 'src-tauri/target/named-dev/debug/domios-server'));
    assert.equal(namedInstanceRoot(directory, data), path.join(directory, 'src-tauri/target/named-dev'));
    rmSync(path.join(data, 'dev-build-provenance.json'));
    assert.equal(namedInstanceRoot(directory, data), path.join(directory, 'src-tauri/target/named-dev'));
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

for (const failure of ['wrong-data', 'auth-failure', 'stale-port', 'other-instance', 'missing-cli', 'request-failure']) {
  test(`full named stop refuses ${failure} with zero StopTree calls`, async t => {
    const checkout = realpathSync.native(mkdtempSync(path.join(tmpdir(), 'named-kill-negative-')));
    t.after(() => rmSync(checkout, { recursive: true, force: true }));
    const root = path.join(checkout, 'src-tauri/target/named-dev');
    const data = path.join(checkout, '.domios-dev-alpha');
    mkdirSync(root, { recursive: true });
    mkdirSync(path.join(data, 'bin'), { recursive: true });
    if (failure !== 'missing-cli') writeFileSync(path.join(data, 'bin', `codeterm${process.platform === 'win32' ? '.exe' : ''}`), 'fixture');
    const server = { pid: 42, created: '123', parent: 1, executable: path.join(root, 'server') };
    const snapshot = { processes: [server], listeners: [{ pid: 42, port: 17996 }] };
    const identityData = ['wrong-data', 'other-instance'].includes(failure) ? `${data}-beta` : data;
    const quitSnapshot = { ready: { process: server, api_address: `127.0.0.1:${failure === 'stale-port' ? 17997 : 17996}`,
      identity: { data: identityData, config: path.join(identityData, 'config.toml') } } };
    const calls = [];
    await assert.rejects(stopInstanceTree(root, 17996, 'alpha', data, {
      checkout,
      inspect: operation => { calls.push(operation); if (operation === 'Inspect') return snapshot; throw Error('Unsafe StopTree'); },
      quit: input => gracefulQuit(input, {
        run: () => ({ status: failure === 'auth-failure' ? 1 : 0, stdout: 'opaque-token' }),
        request: async () => {
          if (failure === 'request-failure') throw Error('request failed');
          return { ok: true, json: async () => quitSnapshot };
        },
      }),
    }), /Uncertain DEV instance identity/);
    assert.deepEqual(calls, ['Inspect']);
  });
}

test('full named stop force-cleans only the authenticated instance tree', async t => {
  const checkout = realpathSync.native(mkdtempSync(path.join(tmpdir(), 'named-kill-positive-')));
  t.after(() => rmSync(checkout, { recursive: true, force: true }));
  const root = path.join(checkout, 'src-tauri/target/named-dev');
  mkdirSync(root, { recursive: true });
  const processes = [11, 21].map(pid => ({ pid, parent: 1, created: String(pid), executable: path.join(root, 'server') }));
  const calls = [];
  await stopInstanceTree(root, 17996, 'alpha', checkout, { checkout, quit: async () => [11],
    inspect: (operation, input) => {
      calls.push(operation);
      if (operation === 'Inspect') return { processes, listeners: [{ pid: 11, port: 17996 }, { pid: 21, port: 17997 }] };
      assert.deepEqual(input.processes, [processes[0]]);
      return { stopped: [] };
    },
  });
  assert.deepEqual(calls, ['Inspect', 'StopTree']);
});
