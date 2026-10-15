import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import { runDevApp } from '../dev-app-process.mjs';
import { devRuntimeEnvironment } from '../dev-runtime-environment.mjs';

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
  const readiness = source.match(/^function readinessSignal\(line, apiPort\) \{[\s\S]*?^\}/m)[0];
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'dev-app-ready-'));
  const messages = [];
  let disconnected = 0;
  const fakeProcess = { env: {}, connected: true, send: value => messages.push(value) };
  const signal = 'REST API generation opaque health-proven on http://127.0.0.1:7786/';
  const script = `const fs=require('node:fs'); if (process.platform !== 'win32' && ![1,2].every(fd => fs.fstatSync(fd).isFile())) process.exit(91); fs.writeSync(2, ${JSON.stringify(signal + '\n')});`;
  const execute = new Function('runDevApp', 'process', 'disconnectSupervisor', 'StringDecoder', 'resolve', 'option', 'repo', 'devRuntimeEnvironment',
    `${readiness}\n${supervisor}\nreturn supervisePrebuilt;`)(
      options => {
        assert.equal(options.env.CODETERM_DEV_WORKSPACE_ROOT, directory);
        return runDevApp({ ...options, args: ['-e', script] });
      }, fakeProcess,
      () => { if (fakeProcess.connected) { fakeProcess.connected = false; disconnected++; } }, StringDecoder, path.resolve,
      (options, key) => options.get(key), directory, devRuntimeEnvironment);
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
