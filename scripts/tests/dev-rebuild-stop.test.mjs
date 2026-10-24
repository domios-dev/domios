import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { rebuildStopPlan } from '../dev-rebuild-stop.mjs';

function fixture() {
  const client = { pid: 42, created: '123', executable: 'C:/target/domios-dev-app.exe', parent: 1 };
  const server = { pid: 43, created: '124', executable: 'C:/target/domios-server/domios-server.exe', parent: 1 };
  const identity = { data: 'C:/Users/owner/.domios-dev-lane', config: 'C:/Users/owner/.domios-dev-lane/config.toml' };
  const ready = { identity, process: server, api_address: '127.0.0.1:17996', epoch: 'epoch', instance_id: 'id' };
  return {
    instance: 'lane', data: 'C:/Users/owner/.domios-dev-lane', port: 17996, app: client.executable,
    record: { schema: 3, instance: { name: 'lane', apiPort: '17996', cdpPort: '17997' },
      artifacts: { app: { path: client.executable }, server: { path: server.executable } } },
    ready, quit: { ready, client: { process: client, connection: { identity, server_epoch: 'epoch', instance_id: 'id' } } },
    processes: [client, server, { pid: 44, created: '125', executable: 'C:/webview.exe', parent: 42 },
      { pid: 50, created: '126', executable: 'C:/prod/domios.exe', parent: 1 }],
    listeners: [{ port: 17996, pid: 43 }, { port: 17997, pid: 44 }, { port: 7685, pid: 50 }],
  };
}

test('running same named DEV selects only the verified client, server and descendants', () => {
  const input = fixture();
  assert.deepEqual(rebuildStopPlan(input).map(row => row.pid), [42, 43, 44]);
});

test('stopped instance requires no provenance and leaves other instances alone', () => {
  const input = fixture();
  input.processes = [input.processes[3]];
  input.listeners = [{ port: 7685, pid: 50 }, { port: 17998, pid: 50 }];
  delete input.record;
  delete input.ready;
  assert.deepEqual(rebuildStopPlan(input), []);
});

test('production port refuses even when stopped', () => {
  assert.throws(() => rebuildStopPlan({ ...fixture(), port: 7685, processes: [], listeners: [] }), /production/);
});

test('another instance name, data directory, port or artifact refuses', () => {
  for (const change of [
    input => { input.instance = 'foreign'; },
    input => { input.data = 'C:/Users/owner/.domios-dev-foreign'; },
    input => { input.record.instance.apiPort = '17998'; },
    input => { input.record.artifacts.app.path = 'C:/foreign/domios-dev-app.exe'; },
    input => { input.ready.process = { ...input.ready.process, executable: 'C:/foreign/server.exe' }; },
    input => { input.ready.identity = { ...input.ready.identity, data: 'C:/foreign' }; },
    input => { input.ready.identity = { ...input.ready.identity, config: 'C:/foreign/config.toml' }; },
    input => { input.listeners[0].pid = 50; },
  ]) {
    const input = fixture(); change(input);
    assert.throws(() => rebuildStopPlan(input), /identity/);
  }
});

test('unknown identity, reused PID and duplicate native client refuse', () => {
  for (const change of [
    input => { delete input.record; },
    input => { delete input.ready; },
    input => { input.quit.client = null; },
    input => { input.quit = { ...input.quit, ready: { ...input.ready, epoch: 'foreign' } }; },
    input => { input.quit.client.connection.server_epoch = 'foreign'; },
    input => { input.quit.client.connection.identity = { data: 'C:/foreign', config: 'C:/foreign/config.toml' }; },
    input => { input.processes[0] = { ...input.processes[0], created: '999' }; },
    input => { input.processes[1] = { ...input.processes[1], created: '999' }; },
    input => { input.processes.push({ ...input.processes[0], pid: 99 }); },
    input => { input.processes[2].created = ''; },
  ]) {
    const input = fixture(); change(input);
    assert.throws(() => rebuildStopPlan(input), /identity/);
  }
});

test('a selected tree containing production or another instance listener refuses', () => {
  for (const port of [7685, 17998]) {
    const input = fixture(); input.listeners.push({ port, pid: 44 });
    assert.throws(() => rebuildStopPlan(input), /another listener/);
  }
});

test('server-only running instance can stop without a stale native snapshot', () => {
  const input = fixture(); input.processes = [input.processes[1]];
  input.listeners = [input.listeners[0]];
  input.quit.client = null;
  assert.deepEqual(rebuildStopPlan(input).map(row => row.pid), [43]);
});

test('launcher stops before both staging and prebuilt launch; preflight refusal preserves DEV', () => {
  const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
  const build = source.match(/^build_and_launch_dev\(\) \{[^]*?^\}/m)[0];
  assert.match(build, /stop_dev_before_replacement \|\| return\s+node scripts\/server-artifact.mjs stage-dev/);
  const prebuilt = source.slice(source.indexOf('\n  dev-prebuilt)\n'), source.indexOf('\n  dev-front)\n'));
  assert.ok(prebuilt.indexOf('validate_dev_provenance') < prebuilt.indexOf('stop_dev_before_replacement'));
  assert.match(prebuilt, /CODETERM_DEV_INSTANCE='\$CT_INSTANCE' CODETERM_DEV_API_PORT='\$CT_PORT' .\/build.sh dev --wait/);
  assert.ok(prebuilt.indexOf('stop_dev_before_replacement') < prebuilt.indexOf('--launch-prebuilt'));
});
