import test from 'node:test';
import {EventEmitter} from 'node:events';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { runDevApp, windowsDevArguments, nativeLocatorRecord, prebuiltReadinessPhase } from '../dev-app-process.mjs';
import { devRuntimeEnvironment } from '../dev-runtime-environment.mjs';
import { rebuildStopPlan } from '../dev-rebuild-stop.mjs';

test('Windows desktop transport delivers runtime frontend identity and excludes inherited credentials', { skip: process.platform !== 'win32', timeout: 20_000 }, () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-desktop-runtime-'));
  try {
    const child = path.join(directory, 'child.cjs');
    const keys = ['CODETERM_VITE_PORT', 'CODETERM_DEV_IDENTIFIER', 'CODETERM_CDP_PORT', 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', 'CODETERM_API_TOKEN', 'CODETERM_API_PORT', 'CODETERM_SOCKET_PATH', 'CT_PRIVATE'];
    fs.writeFileSync(child, `console.log(JSON.stringify(Object.fromEntries(${JSON.stringify(keys)}.map(key => [key, process.env[key] ?? null]))));`);
    const probe = path.join(directory, 'probe.ps1');
    fs.writeFileSync(probe, `
$ErrorActionPreference='Stop'
Add-Type -Path @($env:PROBE_PROCESS_SOURCE, $env:PROBE_DESKTOP_SOURCE)
$env:CODETERM_DEV='1'
$env:CODETERM_VITE_PORT='1851'
$env:CODETERM_DEV_IDENTIFIER='com.domios.dev.transport'
$env:CODETERM_CDP_PORT='9351'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--opaque-runtime-flag'
$env:CODETERM_API_TOKEN='excluded-fixture-token'
$env:CODETERM_API_PORT='7685'
$env:CODETERM_SOCKET_PATH='excluded-fixture-socket'
$env:CT_PRIVATE='excluded-fixture-value'
$flags=[Reflection.BindingFlags]'Static,NonPublic'
$write=[DevDesktop].GetMethod('WriteEnvironment',$flags)
$read=[DevDesktop].GetMethod('ReadEnvironment',$flags)
$buffer=[IO.MemoryStream]::new()
$writer=[IO.BinaryWriter]::new($buffer,[Text.Encoding]::UTF8,$true)
[void]$write.Invoke($null,[object[]]@($writer))
$writer.Flush()
$bytes=$buffer.ToArray()
foreach ($kind in @('count','key')) {
  [byte[]]$broken=$bytes.Clone()
  if ($kind -eq 'count') { [Array]::Clear($broken,0,4) } else { $broken[8]=[byte][char]'X' }
  $invalid=[IO.BinaryReader]::new([IO.MemoryStream]::new($broken))
  try { [void]$read.Invoke($null,[object[]]@($invalid)); throw 'Invalid frame accepted' }
  catch { if ($_.Exception.GetBaseException().Message -notmatch '^DEV environment( key)? refused$') { throw } }
}
$buffer.Position=0
$reader=[IO.BinaryReader]::new($buffer,[Text.Encoding]::UTF8,$true)
$selected=$read.Invoke($null,[object[]]@($reader))
$output=[IO.MemoryStream]::new()
$code=[DevAppProcess]::RunOwned($env:PROBE_NODE,[string[]]@($env:PROBE_CHILD),$output,(Split-Path $env:PROBE_CHILD),$selected,[Threading.CancellationToken]::None)
if ($code -ne 0) { throw "Probe child exit $code" }
[Console]::Write([Text.Encoding]::UTF8.GetString($output.ToArray()))
`);
    const result = spawnSync('powershell.exe', ['-NoProfile', '-File', probe], { encoding: 'utf8', timeout: 20_000,
      env: { ...process.env, PROBE_NODE: process.execPath, PROBE_CHILD: child,
        PROBE_PROCESS_SOURCE: fileURLToPath(new URL('../windows-dev-app.cs', import.meta.url)),
        PROBE_DESKTOP_SOURCE: fileURLToPath(new URL('../windows-dev-desktop.cs', import.meta.url)) } });
    assert.equal(result.status, 0, result.stderr || result.error?.message);
    assert.deepEqual(JSON.parse(result.stdout), { CODETERM_VITE_PORT: '1851', CODETERM_DEV_IDENTIFIER: 'com.domios.dev.transport',
      CODETERM_CDP_PORT: '9351', WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--opaque-runtime-flag',
      CODETERM_API_TOKEN: null, CODETERM_API_PORT: null, CODETERM_SOCKET_PATH: null, CT_PRIVATE: null });
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('DEV shutdown ownership selects the exact native path and preserves foreign and production identities', () => {
  const client = { pid: 42, created: '123', executable: 'C:/selected/domios-dev-app.exe', parent: 1 };
  const server = { pid: 43, created: '124', executable: 'C:/selected/domios-server.exe', parent: 1 };
  const foreign = { pid: 50, created: '125', executable: 'C:/foreign/domios-dev-app.exe', parent: 1 };
  const production = { pid: 60, created: '126', executable: 'C:/production/domios-app.exe', parent: 1 };
  const identity = { data: 'C:/fixture/.domios-dev-selected', config: 'C:/fixture/.domios-dev-selected/config.toml' };
  const ready = { identity, process: server, api_address: '127.0.0.1:17996', epoch: 'epoch', instance_id: 'instance' };
  const input = {
    instance: 'selected', data: identity.data, port: 17996, app: client.executable,
    record: { schema: 3, instance: { name: 'selected', apiPort: '17996', cdpPort: '9496' },
      artifacts: { app: { path: client.executable }, server: { path: server.executable } } },
    ready, quit: { ready, client: { process: client, connection: { identity, server_epoch: 'epoch', instance_id: 'instance' } } },
    processes: [client, server, foreign, production],
    listeners: [{ pid: server.pid, port: 17996 }, { pid: foreign.pid, port: 17997 }, { pid: production.pid, port: 7685 }],
  };
  const before = structuredClone(input);
  assert.deepEqual(rebuildStopPlan(input), [client, server]);
  assert.deepEqual(input, before);
  assert.throws(() => rebuildStopPlan({ ...input, app: foreign.executable }), /identity/);
  assert.throws(() => rebuildStopPlan({ ...input, port: 7685 }), /production/);
  assert.throws(() => rebuildStopPlan({ ...input, processes: [{ ...client, created: '999' }, server, foreign, production] }), /identity/);
});

for (const shutdownExit of [0, 19]) {
  test(`Windows kill awaits exact instance shutdown before sidecar cleanup, exit=${shutdownExit}`, { timeout: 10_000 }, () => {
    const source = fs.readFileSync(new URL('../../build.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
    const definition = source.match(/^kill_dev\(\) \{[^]*?^\}/m)[0];
    const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
    const result = spawnSync(bash, ['-c', `set -euo pipefail
${definition}
OSTYPE=msys CT_PORT=17996 CT_VITE_PORT=14996 CT_CDP_PORT=9496
CT_INSTANCE=selected CT_SUFFIX=-selected CT_DATA_DIR='C:/fixture/.domios-dev-selected'
DEV_FRONT_EXTERNAL=1
CODETERM_TEST_KILL_DRY_RUN=0 CODETERM_TEST_KILL_CLEAN_REQUEST_ONLY=0
dev_provenance_record() { printf '%s' 'C:/fixture/selected/provenance.json'; }
dev_app_path() { printf '%s' 'C:/fixture/selected/domios-dev-app.exe'; }
node() {
  if [ "$1" = scripts/dev-rebuild-stop.mjs ]; then
    printf '%s\\n' "$@"
    return ${shutdownExit}
  fi
  printf '%s\\n' "$@"
}
kill_web_watch() { printf '%s\\n' watcher; }
kill_dev_static_server() { printf '%s\\n' static; }
windows_dev_leftover_cleanup() { printf '%s\\n' leftovers; }
request_dev_clean_restart() { exit 91; }
netstat() { exit 92; }
taskkill() { exit 93; }
wait_for_dev_exit() { exit 94; }
kill_dev
`], { encoding: 'utf8', timeout: 5_000 });
    assert.equal(result.error, undefined);
    assert.equal(result.status, shutdownExit, result.stderr);
    const shutdown = ['scripts/dev-rebuild-stop.mjs', 'C:/fixture/selected/provenance.json',
      'C:/fixture/.domios-dev-selected', '17996', 'selected', 'C:/fixture/selected/domios-dev-app.exe'];
    assert.deepEqual(result.stdout.trim().split(/\r?\n/), shutdownExit === 0
      ? [...shutdown, 'watcher', 'static', 'leftovers'] : shutdown);
  });
}

for (const [flags, survives] of [[0, true], [0x1800, true], [0x2800, false]]) {
  test(`Windows DEV helper job survival, flags=0x${flags.toString(16)}`, { skip: process.platform !== 'win32', timeout: 15_000 }, () => {
    const result = spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
      fileURLToPath(new URL('./fixtures/windows-dev-job.ps1', import.meta.url)),
      '-Root', fileURLToPath(new URL('../../', import.meta.url)), '-Flags', String(flags)], { encoding: 'utf8', timeout: 10_000 });
    assert.equal(result.error, undefined);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), survives ? 'True' : 'False');
  });
}

for (const code of [0, 17]) {
  test(`DEV child stdio is regular files or synchronous pipes, output drains completely, exit=${code}`, { timeout: 10_000 }, async () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-app-stdio-'));
    const log = path.join(directory, 'app.log');
    const chunks = [];
    const script = `
      const fs = require('node:fs');
      if (process.platform !== 'win32' && ![1,2].every(fd => fs.fstatSync(fd).isFile())) process.exit(91);
      fs.writeSync(1, 'file-handles\\n');
      const chunk = Buffer.alloc(65536, 120);
      for (let i = 0; i < 32; i++) fs.writeSync(i % 2 + 1, chunk);
      fs.writeSync(2, '\\nfinal-output\\n');
      process.exit(${code});
    `;
    try {
      const result = await runDevApp({ app: process.execPath, args: ['-e', script], log,
        cwd: directory, env: process.env, onData: chunk => chunks.push(chunk) });
      assert.equal(result.code, code);
      assert.equal(result.signal, null);
      const output = fs.readFileSync(log);
      assert.deepEqual(Buffer.concat(chunks), output);
      assert.equal(output.length, Buffer.byteLength('file-handles\n\nfinal-output\n') + 32 * 65536);
      assert.ok(output.toString().includes('\nfinal-output\n'));
      fs.renameSync(log, path.join(directory, 'closed.log'));
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
}

test('DEV supervisor still sends READY from file output and disconnects exactly once', { timeout: 10_000 }, async () => {
  const source = fs.readFileSync(new URL('../dev-prep.mjs', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const supervisor = source.match(/^async function supervisePrebuilt\(options\) \{[\s\S]*?^\}/m)[0];
  const readiness = source.match(/^function readinessSignal\(line, apiPort\) \{[\s\S]*?^\}/m)[0] + '\n' + source.match(/^function startupFailureSignal\(line\) \{[\s\S]*?^\}/m)[0];
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-app-ready-'));
  const messages = [];
  let disconnected = 0;
  const fakeProcess = { env: {}, connected: true, send: value => messages.push(value) };
  const signal = 'DOMIOS_DEV_ATTACHMENT ' + JSON.stringify({ protocol: 2, client_pid: 10, server_pid: 20, epoch: '00000000-0000-0000-0000-000000000001', api_address: '127.0.0.1:7786' });
  const script = `const fs=require('node:fs'); if (process.platform !== 'win32' && ![1,2].every(fd => fs.fstatSync(fd).isFile())) process.exit(91); fs.writeSync(2, ${JSON.stringify(signal + '\n')});`;
  const execute = new Function('runDevApp', 'process', 'disconnectSupervisor', 'StringDecoder', 'resolve', 'option', 'repo', 'devRuntimeEnvironment', 'prebuiltReadinessPhase',
    `${readiness}\n${supervisor}\nreturn supervisePrebuilt;`)(
      options => {
        assert.equal(options.env.CODETERM_DEV_WORKSPACE_ROOT, directory);
        assert.equal(options.interactiveDesktop, true);
        return runDevApp({ ...options, interactiveDesktop: false, args: ['-e', script] });
      }, fakeProcess,
      () => { if (fakeProcess.connected) { fakeProcess.connected = false; disconnected++; } }, StringDecoder, path.resolve,
      (options, key) => options.get(key), directory, devRuntimeEnvironment, prebuiltReadinessPhase);
  try {
    await execute(new Map([['--app', process.execPath], ['--log', path.join(directory, 'app.log')], ['--api-port', '7786']]));
    assert.deepEqual(messages, [{ type: 'ready', signal }]);
    assert.equal(fakeProcess.exitCode, 0);
    assert.equal(disconnected, 1);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});


test('launcher forwards output while the app is still running', { timeout: 10_000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-app-live-'));
  const ack = path.join(directory, 'ack');
  fs.writeFileSync(ack, 'waiting');
  const script = `const fs = require('node:fs');
    const watcher = fs.watch(${JSON.stringify(ack)}, () => { watcher.close(); process.exit(0); });
    fs.writeSync(1, 'ready-for-ack\\n');`;
  let observed = '';
  try {
    const { code } = await runDevApp({ app: process.execPath, args: ['-e', script],
      log: path.join(directory, 'app.log'), cwd: directory, env: process.env,
      onData: chunk => {
        observed += chunk.toString();
        if (observed.includes('ready-for-ack')) fs.writeFileSync(ack, 'acknowledged');
      } });
    assert.equal(code, 0);
    assert.equal(fs.readFileSync(ack, 'utf8'), 'acknowledged');
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});


test('Windows app standard pipes have synchronous NT handle modes', { skip: process.platform !== 'win32', timeout: 10_000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-app-native-'));
  const probe = path.join(directory, 'probe.ps1');
  fs.writeFileSync(probe, `Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Probe {
  [StructLayout(LayoutKind.Sequential)] public struct Status { public IntPtr Code; public UIntPtr Information; }
  [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int id);
  [DllImport("kernel32.dll")] static extern uint GetFileType(IntPtr handle);
  [DllImport("ntdll.dll")] static extern int NtQueryInformationFile(IntPtr handle, out Status status, out uint mode, uint size, int kind);
  public static string Modes(int id) {
    IntPtr handle = GetStdHandle(id); Status status; uint mode;
    int result = NtQueryInformationFile(handle, out status, out mode, 4, 16);
    return GetFileType(handle) + ":" + result + ":" + mode;
  }
}
'@
[Console]::WriteLine([Probe]::Modes(-11))
[Console]::Error.WriteLine([Probe]::Modes(-12))
`);
  let output = '';
  try {
    const { code } = await runDevApp({ app: 'powershell.exe', args: ['-NoProfile', '-File', probe],
      log: path.join(directory, 'app.log'), cwd: directory, env: process.env,
      onData: chunk => { output += chunk.toString(); } });
    assert.equal(code, 0, output);
    const modes = output.trim().split(/\r?\n/).map(line => line.split(':').map(Number));
    assert.equal(modes.length, 2, output);
    for (const [type, status, mode] of modes) {
      assert.equal(type, 3, 'FILE_TYPE_PIPE');
      assert.equal(status, 0, 'NtQueryInformationFile success');
      assert.notEqual(mode & 0x30, 0, 'FILE_SYNCHRONOUS_IO_ALERT or FILE_SYNCHRONOUS_IO_NONALERT');
    }
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});


test('DEV app launch accepts an empty argument list', { timeout: 10_000 }, async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-app-noargs-'));
  try {
    const result = await runDevApp({ app: process.execPath, log: path.join(directory, 'app.log'),
      cwd: directory, env: process.env });
    assert.equal(result.code, 0, fs.readFileSync(path.join(directory, 'app.log'), 'utf8'));
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('DEV readiness refuses monolith logs and malformed or same-process attachment', () => {
  const source = fs.readFileSync(new URL('../dev-prep.mjs', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const definition = source.match(/^function readinessSignal\(line, apiPort\) \{[\s\S]*?^\}/m)[0];
  const check = new Function('process', `${definition}\nreturn readinessSignal;`)({ env: {} });
  const status = { protocol: 2, client_pid: 10, server_pid: 20, epoch: '00000000-0000-0000-0000-000000000001', api_address: '127.0.0.1:7786' };
  const line = value => 'DOMIOS_DEV_ATTACHMENT ' + JSON.stringify(value);
  assert.equal(check(line(status), '7786'), true);
  assert.equal(check(line({ ...status, api_address: '[::1]:7786' }), '7786'), true);
  for (const value of [{ ...status, protocol: 1 }, { ...status, server_pid: 10 }, { ...status, api_address: '127.0.0.1:7685' }, { ...status, token: 'forbidden' }]) assert.equal(check(line(value), '7786'), false);
  assert.equal(check('REST API generation old health-proven on http://127.0.0.1:7786/', '7786'), false);
});

test('prebuilt attachment proof requires the bounded native event log', () => {
  const source = fs.readFileSync(new URL('../dev-prep.mjs', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const definitions = ['readinessSignal', 'attachmentProof', 'checkAttachment'].map(name => source.match(new RegExp(`^function ${name}\\([^]*?^\\}`, 'm'))[0]).join('\n');
  const check = new Function('process', 'resolve', 'option', 'lstatSync', 'readFileSync', `${definitions}\nreturn checkAttachment;`)(
    { env: {}, kill: pid => { assert.ok([10, 20].includes(pid)); } }, path.resolve, (options, key) => options.get(key), fs.lstatSync, fs.readFileSync);
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-attachment-proof-'));
  try {
    const log = path.join(directory, 'client.log');
    const options = new Map([['--log', log], ['--api-port', '7786'], ['--listener-pid', '20']]);
    fs.writeFileSync(log, 'REST API generation old health-proven on http://127.0.0.1:7786/\n');
    assert.throws(() => check(options), /absent/);
    fs.appendFileSync(log, 'DOMIOS_DEV_ATTACHMENT ' + JSON.stringify({ protocol: 2, client_pid: 10, server_pid: 20, epoch: '00000000-0000-0000-0000-000000000001', api_address: '127.0.0.1:7786' }) + '\n');
    assert.doesNotThrow(() => check(options));
    fs.writeFileSync(log, Buffer.alloc(4 * 1024 * 1024 + 1));
    assert.throws(() => check(options), /bound/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('finite client refusal is surfaced without reflecting error text or secrets', () => {
  const source = fs.readFileSync(new URL('../dev-prep.mjs', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const definition = source.match(/^function startupFailureSignal\(line\) \{[\s\S]*?^\}/m)[0];
  const signal = new Function(`${definition}\nreturn startupFailureSignal;`)();
  const value = { stage: 'startup', status: 'refused', io_kind: null, os_error: null, exit_code: null };
  const line = value => 'ERROR: DOMIOS_DEV_FAILURE ' + JSON.stringify(value);
  assert.equal(signal(line(value)), 'Native client startup refused at startup');
  for (const invalid of [{ ...value, token: 'secret' }, { ...value, stage: 'secret callback-url' }, { ...value, io_kind: 'secret' }, { ...value, status: 'ready' }]) assert.equal(signal(line(invalid)), null);
  assert.equal(signal('ERROR: DOMIOS_DEV_FAILURE malformed secret'), null);
});

test('DEV supervisor reports terminal refusal immediately and cannot turn it into READY', async () => {
  const source = fs.readFileSync(new URL('../dev-prep.mjs', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const definitions = ['readinessSignal', 'startupFailureSignal'].map(name => source.match(new RegExp(`^function ${name}\\([^]*?^\\}`, 'm'))[0]).join('\n');
  const supervisor = source.match(/^async function supervisePrebuilt\(options\) \{[\s\S]*?^\}/m)[0];
  const messages = [];
  const state = { env: {}, connected: true, send: value => messages.push(value) };
  const error = { stage: 'startup', status: 'refused', io_kind: null, os_error: null, exit_code: null };
  const ready = { protocol: 2, client_pid: 1, server_pid: 2, epoch: '00000000-0000-0000-0000-000000000001', api_address: '127.0.0.1:7786' };
  const execute = new Function('runDevApp', 'process', 'disconnectSupervisor', 'StringDecoder', 'resolve', 'option', 'repo', 'devRuntimeEnvironment', 'prebuiltReadinessPhase', `${definitions}\n${supervisor}\nreturn supervisePrebuilt;`)(
    async ({ onData }) => { onData(Buffer.from('ERROR: DOMIOS_DEV_FAILURE ' + JSON.stringify(error) + '\nDOMIOS_DEV_ATTACHMENT ' + JSON.stringify(ready) + '\n')); return { code: 0, signal: null }; },
    state, () => { state.connected = false; }, StringDecoder, path.resolve, (options, key) => options.get(key), process.cwd(), env => env, prebuiltReadinessPhase);
  await execute(new Map([['--app', process.execPath], ['--log', 'fixture.log'], ['--api-port', '7786']]));
  assert.deepEqual(messages, [{ type: 'failed', error: 'Native client startup refused at startup' }]);
  assert.equal(state.exitCode, 1);
});


test('interactive Windows DEV invocation is finite and keeps selectors out of argv', () => {
  const input = { wrapper: 'C:/repo with spaces/scripts/windows-dev-app.ps1', app: 'C:/target/debug/codeterm.exe',
    args: [], interactiveDesktop: true, env: { CODETERM_DEV: '1', CODETERM_API_TOKEN: 'PRIVATE_TOKEN', CODETERM_SOCKET_PATH: 'PRIVATE_SOCKET' } };
  const argv = windowsDevArguments(input);
  assert.deepEqual(argv, ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', input.wrapper, '-App', input.app, '-InteractiveDesktop']);
  assert.doesNotMatch(JSON.stringify(argv), /PRIVATE_TOKEN|PRIVATE_SOCKET/);
  assert.throws(() => windowsDevArguments({ ...input, args: ['workspace'] }), /no arbitrary/);
  assert.throws(() => windowsDevArguments({ ...input, env: {} }), /explicit development intent/);
  assert.deepEqual(windowsDevArguments({ ...input, interactiveDesktop: false, args: ['fixture'] }), argv.slice(0, -1));
});


test('split attachment proof refuses stale listener and dead native owner', () => {
  const source = fs.readFileSync(new URL('../dev-prep.mjs', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
  const definitions = ['readinessSignal', 'attachmentProof'].map(name => source.match(new RegExp(`^function ${name}\\([^]*?^\\}`, 'm'))[0]).join('\n');
  const proof = new Function('process', `${definitions}\nreturn attachmentProof;`)({ env: {} });
  const event = { protocol: 2, client_pid: 10, server_pid: 20, epoch: '00000000-0000-0000-0000-000000000001', api_address: '127.0.0.1:7786' };
  const lines = ['DOMIOS_DEV_ATTACHMENT ' + JSON.stringify(event)];
  assert.deepEqual(proof(lines, '7786', '20', () => true), event);
  for (const listener of ['', '0', '21']) assert.throws(() => proof(lines, '7786', listener, () => true), /listener/);
  for (const dead of [10, 20]) assert.throws(() => proof(lines, '7786', '20', pid => pid !== dead), /no longer live/);
  assert.throws(() => proof(lines, '7685', '20', () => true), /absent/);
});

test('native DEV locator binds active endpoint to selected instance and exact attached client',()=>{
  const record={schema:3,instance:{cdpPort:'9341',apiPort:'7786',vitePort:'1531'}};
  const attachment={client_pid:12,epoch:'epoch'};
  const snapshot={client_process:{pid:12,created:'12345',executable:'C:/target/debug/codeterm.exe'},connection:{server_epoch:'epoch'},status:{status:'active',payload:{port:9341,view_generation:2}}};
  const event={snapshot,navigation_generation:3,origin:'http://127.0.0.1:1531'};
  const line=value=>'DOMIOS_DEV_NATIVE_LOCATOR '+JSON.stringify(value);
  const result=nativeLocatorRecord(line(event),record,'C:/target/debug/codeterm.exe',attachment);
  assert.deepEqual(result.nativeClient,{platform:'windows',snapshot,navigationGeneration:3,origin:event.origin});
  for (const bad of [{...event,navigation_generation:0},{...event,origin:'http://127.0.0.1:7685'},
    {...event,snapshot:{...snapshot,client_process:{...snapshot.client_process,pid:99}}},
    {...event,snapshot:{...snapshot,status:{status:'active',payload:{port:9222,view_generation:2}}}}])
    assert.throws(()=>nativeLocatorRecord(line(bad),record,'C:/target/debug/codeterm.exe',attachment));
  assert.throws(()=>nativeLocatorRecord(line(event),record,'C:/foreign/codeterm.exe',attachment));
  assert.throws(()=>nativeLocatorRecord(line(event),record,'C:/target/debug/codeterm.exe',{...attachment,epoch:'stale'}));
  assert.equal(nativeLocatorRecord(line({...event,snapshot:{...snapshot,status:{status:'unsupported'}}}),record,'C:/target/debug/codeterm.exe',attachment).nativeClient,null);
});

test('native locator provenance clears previous launch and waits exact attachment before atomic adoption',()=>{
  const source=fs.readFileSync(new URL('../dev-prep.mjs',import.meta.url),'utf8').replaceAll('\r\n','\n');
  const definitions=['readinessSignal','nativeLocatorSink'].map(name=>source.match(new RegExp(`^function ${name}\\([^]*?^\\}`, 'm'))[0]).join('\n');
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'native-dev-locator-'));
  const app=path.join(directory,'codeterm.exe'), record=path.join(directory,'provenance.json');
  fs.writeFileSync(app,'fixture');
  fs.writeFileSync(record,JSON.stringify({schema:3,instance:{apiPort:'7786',vitePort:'1531',cdpPort:'9341'},nativeClient:{stale:true}}));
  try {
    const factory=new Function('resolve','option','realpathSync','StringDecoder','process','writeFileSync','renameSync','readFileSync','nativeLocatorRecord',`${definitions}\nreturn nativeLocatorSink;`)(path.resolve,(options,key)=>options.get(key),fs.realpathSync,StringDecoder,process,fs.writeFileSync,fs.renameSync,fs.readFileSync,nativeLocatorRecord);
    const consume=factory(new Map([['--app',app],['--record',record],['--api-port','7786']]));
    assert.equal(JSON.parse(fs.readFileSync(record)).nativeClient,null);
    const event={snapshot:{client_process:{pid:12,created:'12345',executable:app},connection:{server_epoch:'00000000-0000-0000-0000-000000000001'},status:{status:'active',payload:{port:9341,view_generation:2}}},navigation_generation:3,origin:'http://127.0.0.1:1531'};
    const line='DOMIOS_DEV_NATIVE_LOCATOR '+JSON.stringify(event)+'\n';
    consume(Buffer.from(line.slice(0,30))); consume(Buffer.from(line.slice(30)));
    assert.equal(JSON.parse(fs.readFileSync(record)).nativeClient,null);
    consume(Buffer.from('DOMIOS_DEV_ATTACHMENT '+JSON.stringify({protocol:2,client_pid:12,server_pid:24,epoch:event.snapshot.connection.server_epoch,api_address:'127.0.0.1:7786'})+'\n'));
    assert.deepEqual(JSON.parse(fs.readFileSync(record)).nativeClient.snapshot,event.snapshot);
    assert.deepEqual(fs.readdirSync(directory).sort(),['codeterm.exe','provenance.json']);
  } finally {fs.rmSync(directory,{recursive:true,force:true});}
});

test('prebuilt readiness phases classify absent attach then launch without treating progress as readiness',()=>{
  const line=stage=>'DOMIOS_CLIENT_DIAGNOSTIC '+JSON.stringify({stage,status:'started',exit_code:null,io_kind:null,os_error:null});
  let phase=0;
  assert.equal(prebuiltReadinessPhase(phase,line('reply_validation')),0);
  phase=prebuiltReadinessPhase(phase,line('attach')); assert.equal(phase,1);
  for (const stage of ['attach','launch','inventory']) assert.equal(prebuiltReadinessPhase(phase,line(stage)),1);
  phase=prebuiltReadinessPhase(phase,line('reply_validation')); assert.equal(phase,2);
  assert.equal(prebuiltReadinessPhase(phase,line('reply_validation')),2);
  assert.equal(prebuiltReadinessPhase(phase,'DOMIOS_DEV_ATTACHMENT malformed'),2);
});

test('prebuilt launch partitions bounded verification bootstrap and attachment without duplicate progress resets',async()=>{
  const source=fs.readFileSync(new URL('../dev-prep.mjs',import.meta.url),'utf8').replaceAll('\r\n','\n');
  const definition=source.match(/^async function launchPrebuilt\(options\) \{[\s\S]*?^\}/m)[0].replaceAll('import.meta.url',JSON.stringify(import.meta.url));
  const timers=[],cleared=[];
  let stopped=0;
  let now=0;
  const readiness=Promise.withResolvers(), checking=Promise.withResolvers();
  let complete=false;
  let startupSignal;
  const launch=new Function('resolve','option','process','existsSync','statSync','spawn','node','fileURLToPath','repo','console','setTimeout','clearTimeout','terminateOwnedChild','waitForReady',`${definition}\nreturn launchPrebuilt;`)(path.resolve,(options,key)=>options.get(key),{env:{}},()=>true,()=>({isFile:()=>true}),()=>{
    const child=new EventEmitter();child.pid=123;child.unref=()=>{};
    queueMicrotask(()=>{
      now=29000;
      child.emit('message',{type:'phase',phase:2});
      child.emit('message',{type:'phase',phase:1});
      now=58000;
      child.emit('message',{type:'phase',phase:1});
      child.emit('message',{type:'phase',phase:2});
      child.emit('message',{type:'phase',phase:2});
      now=59000;
      child.emit('message',{type:'ready',signal:'validated attachment'});
    });
    return child;
  },'node',()=>'/fixture/dev-prep.mjs','/fixture',{log(){}},(callback,ms)=>{const timer={callback,ms,expires:now+ms};timers.push(timer);return timer;},timer=>cleared.push(timer),async()=>{stopped++;},options=>{
    assert.equal(options.origin,'http://127.0.0.1:7686');startupSignal=options.signal;checking.resolve();return readiness.promise;
  });
  const result=launch(new Map([['--app','/fixture/codeterm.exe'],['--log','/fixture/log'],['--api-port','7686']])).then(()=>{complete=true;});
  await checking.promise;
  assert.equal(complete,false);
  assert.equal(cleared.length,2);
  readiness.resolve();
  await result;
  assert.equal(startupSignal.aborted,true);
  assert.equal(timers.length,3);assert.ok(timers.every(timer=>timer.ms===30000));
  assert.deepEqual(timers.map(timer=>timer.expires),[30000,59000,88000]);
  assert.deepEqual(cleared,timers);assert.equal(stopped,0);
});

for (const externalFrontend of [false, true]) {
  test(`prebuilt attachment deadline retains processes only for split DEV=${externalFrontend}`, async () => {
    const source = fs.readFileSync(new URL('../dev-prep.mjs', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
    const definition = source.match(/^async function launchPrebuilt\(options\) \{[\s\S]*?^\}/m)[0].replaceAll('import.meta.url', JSON.stringify(import.meta.url));
    const timers = [];
    let stopped = 0, disconnected = 0, readinessChecks = 0;
    const child = new EventEmitter();
    Object.assign(child, { pid: 123, connected: true, unref() {}, disconnect() { disconnected++; this.connected = false; } });
    const launch = new Function('resolve', 'option', 'process', 'existsSync', 'statSync', 'spawn', 'node', 'fileURLToPath', 'repo', 'console', 'setTimeout', 'clearTimeout', 'terminateOwnedChild', 'waitForReady', `${definition}\nreturn launchPrebuilt;`)(
      path.resolve, (options, key) => options.get(key), { env: { DEV_FRONT_EXTERNAL: externalFrontend ? '1' : '0' } },
      () => true, () => ({ isFile: () => true }), () => child, 'node', () => '/fixture/dev-prep.mjs', '/fixture', { log() {} },
      (callback, ms) => { const timer = { callback, ms }; timers.push(timer); return timer; }, () => {},
      async () => { stopped++; }, async () => { readinessChecks++; });
    const result = launch(new Map([['--app', '/fixture/app'], ['--log', '/fixture/log'], ['--api-port', '17886']]));
    child.emit('message', { type: 'phase', phase: 1 });
    child.emit('message', { type: 'phase', phase: 2 });
    assert.deepEqual(timers.map(timer => timer.ms), [30_000, 30_000, externalFrontend ? 180_000 : 30_000]);
    timers.at(-1).callback();
    await assert.rejects(result, error => {
      assert.match(error.message, /phase=attachment/);
      assert.equal(error.code, externalFrontend ? 'DEV_ATTACHMENT_PENDING' : undefined);
      return true;
    });
    assert.equal(stopped, externalFrontend ? 0 : 1);
    assert.equal(disconnected, externalFrontend ? 1 : 0);
    assert.equal(readinessChecks, 0);
    assert.deepEqual(child.eventNames(), []);
  });
}
