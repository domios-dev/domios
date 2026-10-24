import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
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

test('owned browser and provider descendants may listen on their own ports', () => {
  const input = fixture();
  input.processes.push(
    { pid: 60, parent: 43, created: '130', executable: 'C:/Chrome/chrome.exe' },
    { pid: 61, parent: 43, created: '131', executable: 'C:/shell.exe' },
    { pid: 62, parent: 61, created: '132', executable: 'C:/terminal.exe' },
    { pid: 63, parent: 62, created: '133', executable: 'C:/opencode.exe' });
  input.listeners.push({ pid: 60, port: 65456 }, { pid: 63, port: 54173 });
  assert.deepEqual(rebuildStopPlan(input).map(row => row.pid), [42, 43, 44, 60, 61, 62, 63]);
});

test('loopback and tailnet IPv4 and IPv6 listeners identify one server owner', () => {
  const input = fixture();
  input.listeners = input.listeners.filter(row => row.port !== input.port);
  for (const address of ['127.0.0.1', '100.84.159.37', 'fd7a:115c:a1e0::9001:9f33']) {
    input.listeners.push({ pid: 43, port: input.port, address });
  }
  assert.deepEqual(rebuildStopPlan(input).map(row => row.pid), [42, 43, 44]);
  input.listeners.push({ pid: 50, port: input.port, address: '::1' });
  assert.throws(() => rebuildStopPlan(input), /identity/);
});

test('production listeners and foreign ports on verified roots refuse', () => {
  for (const [pid, port] of [[44, 7685], [43, 7685], [43, 17998], [42, 17998]]) {
    const input = fixture(); input.listeners.push({ port, pid });
    assert.throws(() => rebuildStopPlan(input), /production or foreign root listener/);
  }
});

test('another Domios instance nested in the selected tree refuses', () => {
  for (const executable of ['C:/other/domios-server.exe', 'C:/other/domios-dev-app.exe', 'C:/prod/domios-app.exe']) {
    const input = fixture();
    input.processes.push({ pid: 60, parent: 44, created: '130', executable });
    input.listeners.push({ pid: 60, port: 17998 });
    assert.throws(() => rebuildStopPlan(input), /another instance/);
  }
});

test('same-image PTY helpers and other non-serve descendants stop with the verified server', () => {
  for (const commandLine of [
    'domios-server.exe pty-child powershell.exe -NoLogo',
    '"C:\\target\\domios-server\\domios-server.exe" pty-child "C:\\Program Files\\shell.exe" serve',
    '"C:\\Program Files\\Domios\\domios-server.exe" "pty-child" shell.exe',
    'domios-server.exe observe --pipe opaque',
  ]) {
    const input = fixture();
    input.processes.push(
      { pid: 60, parent: 43, created: '130', executable: 'C:/shell.exe' },
      { pid: 61, parent: 60, created: '131', executable: input.ready.process.executable, commandLine },
      { pid: 62, parent: 61, created: '132', executable: 'C:/shell.exe' });
    assert.deepEqual(rebuildStopPlan(input).map(row => row.pid), [42, 43, 44, 60, 61, 62], commandLine);
  }
});

test('same-image serve descendants refuse even before they own a listener', () => {
  for (const commandLine of ['domios-server.exe serve', '"C:\\target\\domios-server\\domios-server.exe" "serve" --pipe pty-child']) {
    const input = fixture();
    input.processes.push({ pid: 60, parent: 43, created: '130', executable: input.ready.process.executable, commandLine });
    assert.throws(() => rebuildStopPlan(input), /another instance/);
  }
});

test('helper exemption refuses listener owners, foreign images, client children and unknown subcommands', () => {
  for (const change of [
    (input, child) => { input.listeners.push({ pid: child.pid, port: 17998 }); },
    (input, child) => { input.listeners.push({ pid: child.pid, port: 7685 }); },
    (_input, child) => { child.executable = 'C:/foreign/domios-server.exe'; },
    (_input, child) => { child.parent = 42; },
    (_input, child) => { delete child.commandLine; },
    (_input, child) => { child.commandLine = 'domios-server.exe'; },
    (_input, child) => { child.commandLine = 'domios-server.exe --pipe pty-child'; },
  ]) {
    const input = fixture();
    const child = { pid: 60, parent: 43, created: '130', executable: input.ready.process.executable,
      commandLine: 'domios-server.exe pty-child shell.exe' };
    change(input, child);
    input.processes.push(child);
    assert.throws(() => rebuildStopPlan(input), /another instance/);
  }
});

test('Windows snapshot captures the process command line for helper classification', { skip: process.platform !== 'win32' }, () => {
  const source = readFileSync(new URL('../windows-dev-rebuild-stop.ps1', import.meta.url), 'utf8');
  const helper = fileURLToPath(new URL('../windows-dev-rebuild-process.cs', import.meta.url));
  const snapshot = source.match(/^function Snapshot[^]*?^\}/m)[0];
  const command = `Add-Type -Path '${helper.replaceAll("'", "''")}'
${snapshot}
function Get-NetTCPConnection { @() }
function Get-CimInstance { [pscustomobject]@{ ProcessId = $PID; ParentProcessId = 1; ExecutablePath = (Join-Path $PSHOME 'powershell.exe'); CommandLine = 'server.exe pty-child shell.exe' } }
$row = (Snapshot).processes[0]
if ($row.commandLine -cne 'server.exe pty-child shell.exe' -or $row.created -notmatch '^[0-9]+$' -or
    $row.imageIdentity -cne [DevRebuildProcess]::FileIdentity($row.executable)) { exit 1 }
`;
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(command, 'utf16le').toString('base64')], {
    encoding: 'utf8', windowsHide: true, timeout: 10_000,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
});

test('Windows final process guard uses file identity across aliases and moves and refuses identity changes', { skip: process.platform !== 'win32' }, () => {
  const source = readFileSync(new URL('../windows-dev-rebuild-stop.ps1', import.meta.url), 'utf8');
  const guard = source.match(/^function Assert-ProcessIdentity\([^]*?^\}/m)[0];
  const row = { pid: 44, created: '125', executable: 'C:/launcher/bin/agent.exe', imageIdentity: '01234567:0000000000000001' };
  const cases = [
    { name: 'junction', process: { ...row, Id: 44, executable: 'C:/packages/release/bin/agent.exe' }, ok: true },
    { name: 'moved-install', process: { ...row, Id: 44, executable: 'C:/moved/bin/agent.exe' }, ok: true },
    { name: 'different-creation', process: { ...row, Id: 44, created: '999' }, ok: false },
    { name: 'different-file', process: { ...row, Id: 44, imageIdentity: '01234567:0000000000000002' }, ok: false },
    { name: 'different-volume', process: { ...row, Id: 44, imageIdentity: '76543210:0000000000000001' }, ok: false },
    { name: 'different-pid', process: { ...row, Id: 45 }, ok: false },
    { name: 'missing-file-identity', process: { ...row, Id: 44 }, row: { ...row, imageIdentity: '' }, ok: false },
  ];
  const command = `$ErrorActionPreference = 'Stop'
${guard}
$row = '${JSON.stringify(row)}' | ConvertFrom-Json
$cases = '${JSON.stringify(cases)}' | ConvertFrom-Json
foreach ($case in $cases) {
  $expected = $row
  if ($case.row) { $expected = $case.row }
  $accepted = $true
  try { Assert-ProcessIdentity $case.process $expected } catch { $accepted = $false }
  if ($accepted -ne $case.ok) { throw "Wrong identity decision: $($case.name)" }
}
Write-Output 'identity cases=7 passed'
`;
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(command, 'utf16le').toString('base64')], {
    encoding: 'utf8', windowsHide: true, timeout: 10_000,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /identity cases=7 passed/);
});

test('Windows final listener guard accepts captured descendants and refuses changed ownership', { skip: process.platform !== 'win32' }, () => {
  const source = readFileSync(new URL('../windows-dev-rebuild-stop.ps1', import.meta.url), 'utf8');
  const guard = source.match(/^function Assert-Listeners\([^]*?^\}/m)[0];
  const allowed = [{ pid: 43, port: 17996 }, { pid: 60, port: 65456 }, { pid: 63, port: 54173 }];
  const cases = [
    { listeners: allowed, allowed, ok: true },
    { listeners: [...allowed, allowed[0]], allowed, ok: true },
    { listeners: [...allowed, { pid: 50, port: 17998 }], allowed, ok: true },
    { listeners: [...allowed, { pid: 50, port: 17996 }], allowed, ok: false },
    { listeners: [...allowed, { pid: 60, port: 65457 }], allowed, ok: false },
    { listeners: [...allowed, { pid: 63, port: 7685 }], allowed: [...allowed, { pid: 63, port: 7685 }], ok: false },
  ];
  const command = `${guard}\n$cases = '${JSON.stringify(cases)}' | ConvertFrom-Json\nforeach ($case in $cases) {\n  $accepted = $true\n  try { Assert-Listeners $case.listeners @(42,43,60,63) $case.allowed 17996 } catch { $accepted = $false }\n  if ($accepted -ne $case.ok) { exit 1 }\n}\nWrite-Output 'listener cases=6 passed'\n`;
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(command, 'utf16le').toString('base64')], {
    encoding: 'utf8', windowsHide: true, timeout: 10_000,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /listener cases=6 passed/);
});

test('server-only running instance can stop without a stale native snapshot', () => {
  const input = fixture(); input.processes = [input.processes[1]];
  input.listeners = [input.listeners[0]];
  input.quit.client = null;
  assert.deepEqual(rebuildStopPlan(input).map(row => row.pid), [43]);
});

test('Windows pinned shutdown waits through termination races and refuses surviving processes', { skip: process.platform !== 'win32' }, () => {
  const helper = fileURLToPath(new URL('../windows-dev-rebuild-process.cs', import.meta.url));
  const command = `
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
${readFileSync(helper, 'utf8')}
public sealed class FakeDevRebuildProcess : IDevRebuildProcess {
    public bool AlreadyExited, ExitsAfterKill;
    public int KillError, Kills, Waits, LastBound;
    public bool WaitForExit(int milliseconds) {
        Waits++;
        LastBound = milliseconds;
        return AlreadyExited || (milliseconds > 0 && ExitsAfterKill);
    }
    public void Kill() {
        Kills++;
        if (KillError != 0) throw new System.ComponentModel.Win32Exception(KillError);
    }
}
'@
$cases = @(
  @{ name='already-exited'; already=$true; exits=$false; error=0; ok=$true; kills=0; waits=1 },
  @{ name='exited-during-kill'; already=$false; exits=$true; error=0; ok=$true; kills=1; waits=2 },
  @{ name='still-running'; already=$false; exits=$false; error=0; ok=$false; kills=1; waits=2 },
  @{ name='access-denied-then-exited'; already=$false; exits=$true; error=5; ok=$true; kills=1; waits=2 },
  @{ name='access-denied-still-running'; already=$false; exits=$false; error=5; ok=$false; kills=1; waits=2 },
  @{ name='other-kill-error'; already=$false; exits=$true; error=6; ok=$false; kills=1; waits=1 }
)
foreach ($case in $cases) {
  $fake = New-Object FakeDevRebuildProcess
  $fake.AlreadyExited = $case.already
  $fake.ExitsAfterKill = $case.exits
  $fake.KillError = $case.error
  $accepted = $true
  try { [DevRebuildProcess]::Stop($fake, 1234) } catch {
    $accepted = $false
    if ($case.waits -eq 2 -and $_.Exception.ToString() -notmatch 'did not exit; refusing staging') { throw }
    if ($case.error -eq 6 -and $_.Exception.InnerException.NativeErrorCode -ne 6) { throw }
  }
  if ($accepted -ne $case.ok -or $fake.Kills -ne $case.kills -or $fake.Waits -ne $case.waits) {
    throw "Wrong shutdown decision: $($case.name)"
  }
  if ($case.waits -eq 2 -and $fake.LastBound -ne 1234) { throw 'Exit wait lost its bound' }
}
$self = [DevRebuildProcess]::Open($PID)
$process = [Diagnostics.Process]::GetCurrentProcess()
try {
  if ($self.Id -ne $PID -or $self.WaitForExit(0) -or
      $self.Created -ne $process.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() -or
      $self.Executable -ine $process.MainModule.FileName -or
      $self.ImageIdentity -cne [DevRebuildProcess]::FileIdentity($process.MainModule.FileName)) { throw 'Pinned handle identity mismatch' }
} finally { $self.Dispose(); $process.Dispose() }
$directory = [IO.Directory]::CreateDirectory((Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString()))).FullName
$original = Join-Path $directory 'image.exe'
$alias = Join-Path $directory 'alias.exe'
$moved = Join-Path $directory 'moved.exe'
try {
  [IO.File]::WriteAllText($original, 'opaque image fixture')
  $identity = [DevRebuildProcess]::FileIdentity($original)
  New-Item -ItemType HardLink -Path $alias -Target $original | Out-Null
  if ([DevRebuildProcess]::FileIdentity($alias) -cne $identity) { throw 'Alias changed file identity' }
  [IO.File]::Move($original, $moved)
  if ([DevRebuildProcess]::FileIdentity($moved) -cne $identity) { throw 'Move changed file identity' }
  [IO.File]::WriteAllText($original, 'opaque image fixture')
  if ([DevRebuildProcess]::FileIdentity($original) -ceq $identity) { throw 'Replacement reused file identity' }
} finally {
  foreach ($file in @($original, $alias, $moved)) { if ([IO.File]::Exists($file)) { [IO.File]::Delete($file) } }
  [IO.Directory]::Delete($directory)
}
Write-Output 'shutdown cases=6 and native pinned identity passed'
`;
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(command, 'utf16le').toString('base64')], {
    encoding: 'utf8', windowsHide: true, timeout: 10_000,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /shutdown cases=6 and native pinned identity passed/);
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
