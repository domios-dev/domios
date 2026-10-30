import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { StringDecoder } from 'node:string_decoder';
import { fileURLToPath } from 'node:url';

const source = fs.readFileSync(new URL('../dev-prep.mjs', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const definition = name => source.match(new RegExp(`^(?:async )?function ${name}\\([^]*?^\\}`, 'm'))[0];

test('supervisor reports an injected locator exception with its stack to the launcher and the log', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-supervisor-failure-'));
  const log = path.join(directory, 'prebuilt.log');
  const messages = [], stderr = [];
  const supervisorProcess = { env: {}, connected: true, send: message => messages.push(message), stderr: { write: text => stderr.push(text) } };
  try {
    const supervise = new Function('resolve', 'option', 'StringDecoder', 'nativeLocatorSink', 'runDevApp', 'repo', 'devRuntimeEnvironment',
      'process', 'prebuiltReadinessPhase', 'startupFailureSignal', 'readinessSignal', 'disconnectSupervisor', 'appendFileSync',
      `${definition('supervisorFailureDetail')}\n${definition('supervisePrebuilt')}\nreturn supervisePrebuilt;`)(
      path.resolve, (options, key) => options.get(key), StringDecoder,
      () => () => { throw Error('opaque-locator-fault'); },
      async ({ onData }) => { onData(Buffer.from('fixture output\n')); return { code: 0, signal: null }; },
      directory, () => ({}), supervisorProcess, phase => phase, () => null, () => false,
      () => { supervisorProcess.connected = false; }, fs.appendFileSync);
    await supervise(new Map([['--app', '/fixture/app'], ['--log', log], ['--api-port', '17887'], ['--record', '/fixture/record']]));
    assert.equal(messages.length, 1);
    assert.equal(messages[0].type, 'launch-failed');
    assert.match(messages[0].error, /opaque-locator-fault/);
    assert.match(messages[0].error, /\n\s+at /);
    assert.match(fs.readFileSync(log, 'utf8'), /\[dev-prep supervisor\] Error: opaque-locator-fault/);
    assert.match(stderr.join(''), /opaque-locator-fault/);
    assert.equal(supervisorProcess.exitCode, 1);
    assert.equal(supervisorProcess.connected, false);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('launcher failure output carries the supervisor locator exception and stack', { skip: process.platform === 'win32', timeout: 20_000 }, () => {
  const directory = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'dev-launcher-failure-')));
  const app = path.join(directory, 'app.sh'), log = path.join(directory, 'prebuilt.log'), record = path.join(directory, 'provenance.json');
  const attachment = { protocol: 2, client_pid: 10, server_pid: 20, epoch: '00000000-0000-0000-0000-000000000001', api_address: '127.0.0.1:17888' };
  const locator = { snapshot: { client_process: { pid: 99, created: '1', executable: app }, connection: { server_epoch: attachment.epoch }, status: { status: 'disabled' } },
    navigation_generation: 1, origin: 'http://127.0.0.1:17888' };
  fs.writeFileSync(app, `#!/bin/sh\nprintf '%s\\n' '${`DOMIOS_DEV_NATIVE_LOCATOR ${JSON.stringify(locator)}`}' '${`DOMIOS_DEV_ATTACHMENT ${JSON.stringify(attachment)}`}'\nexec sleep 30\n`, { mode: 0o755 });
  fs.writeFileSync(record, JSON.stringify({ schema: 3, instance: { apiPort: '17888', vitePort: '17889', cdpPort: '17890' } }));
  const environment = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('CODETERM_')));
  try {
    const result = spawnSync(process.execPath, [fileURLToPath(new URL('../dev-prep.mjs', import.meta.url)), '--launch-prebuilt',
      '--app', app, '--log', log, '--api-port', '17888', '--record', record], { encoding: 'utf8', env: environment, timeout: 15_000 });
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /\[dev-prep\] Error: Native DEV locator identity mismatch/);
    assert.match(result.stderr, /at nativeLocatorRecord/);
    assert.match(fs.readFileSync(log, 'utf8'), /\[dev-prep supervisor\] Error: Native DEV locator identity mismatch/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
