// WHAT: exercise real isolated server lifetime and first-launch arbitration.
// WHY: source models cannot prove OS ownership, private IPC or PTY survival.
// ORACLE: authenticated same-epoch readiness and exact process-exit handles, with one initialization audit.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID, createHash } from 'node:crypto';
import { once } from 'node:events';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, symlinkSync, watch, writeFileSync } from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertServerClosure, SERVER_FEATURES } from './server-artifact.mjs';

const deadline = () => AbortSignal.timeout(45000);
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const LEGACY = ['code', 'term'].join('');
export function parseProbeArgs(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i], value = argv[i + 1];
    if (!['--case', '--instance', '--artifact-receipt', '--client-receipt'].includes(flag) || !value || options[flag]) {
      throw new Error('Expected --case server-only|arbitration|pair-journey --instance split-oracle');
    }
    options[flag] = value;
    i += 1;
  }
  if (!['server-only', 'arbitration', 'pair-journey'].includes(options['--case']) || options['--instance'] !== 'split-oracle') {
    throw new Error('Only the explicit disposable split-oracle instance is permitted');
  }
  return { case: options['--case'], receipt: options['--artifact-receipt'] ?? null, clientReceipt: options['--client-receipt'] ?? null };
}
export function cleanEnvironment(environment, isolatedHome) {
  if (!isolatedHome) throw new Error('A disposable home directory is required for process probes');
  const safeHome = assertDisposableHome(isolatedHome);
  const clientContract = clientEnvironmentContract(readFileSync(path.join(REPO, 'src-tauri/src/run.rs'), 'utf8'));
  const productPrefix = clientContract.instance.replace(/(?:_DEV)?_INSTANCE$/, '').toUpperCase();
  const serverLaunch = readFileSync(path.join(REPO, 'src-tauri/crates/domios-server/src/launch.rs'), 'utf8');
  const ownerVariable = serverLaunch.match(/"([A-Z][A-Z0-9_]*_MACHINE_OWNER)"/)?.[1];
  if (!ownerVariable) throw new Error('Server machine-owner environment contract is absent');
  const result = Object.fromEntries(Object.entries(environment).filter(([key]) => {
    const upper = key.toUpperCase();
    return !upper.startsWith(`${LEGACY.toUpperCase()}_`) && !upper.startsWith(`${productPrefix}_`)
      && !/(?:_INSTANCE|_API_PORT|_API_URL|_API_TOKEN|_DATA_DIR|_SOCKET_PATH)$/.test(upper)
      && !['DISPLAY', 'WAYLAND_DISPLAY', 'TAURI_DEV_HOST'].includes(upper);
  }));
  const home = safeHome;
  const temp = path.join(home, 'tmp');
  for (const directory of [home, temp, path.join(home, 'AppData/Roaming'), path.join(home, 'AppData/Local'),
    path.join(home, '.config'), path.join(home, '.local/share'), path.join(home, '.cache')]) mkdirSync(directory, { recursive: true });
  Object.assign(result, {
    HOME: home, USERPROFILE: home, APPDATA: path.join(home, 'AppData/Roaming'), LOCALAPPDATA: path.join(home, 'AppData/Local'),
    TMP: temp, TEMP: temp, TMPDIR: temp, XDG_CONFIG_HOME: path.join(home, '.config'),
    XDG_DATA_HOME: path.join(home, '.local/share'), XDG_CACHE_HOME: path.join(home, '.cache'),
  });
  result[ownerVariable] = 'false';
  return result;
}
export function assertSafePort(port) {
  if (!Number.isInteger(port) || port < 1 || port > 65535 || port === 7685 || port === 7686) {
    throw new Error(`Refused unsafe process-probe port ${port}`);
  }
  return port;
}
export function withinDirectory(root, candidate) {
  const relative = path.relative(path.resolve(root), path.resolve(candidate));
  return relative !== '' && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}
function cargoSections(manifestText) {
  const headings = [...manifestText.matchAll(/^\[+(.+?)\]+[ \t]*$/gm)];
  return headings.map((heading, index) => ({
    name: heading[1],
    body: manifestText.slice(heading.index + heading[0].length, headings[index + 1]?.index ?? manifestText.length),
  }));
}
export function clientBuildTarget(manifestText) {
  const sections = cargoSections(manifestText);
  const packageSection = sections.find((section) => section.name === 'package')?.body ?? '';
  const packageName = packageSection.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
  const defaultRun = packageSection.match(/^default-run\s*=\s*"([^"]+)"/m)?.[1];
  const bins = sections.filter((section) => section.name === 'bin').map((section) => {
    return { name: section.body.match(/^name\s*=\s*"([^"]+)"/m)?.[1], source: section.body.match(/^path\s*=\s*"([^"]+)"/m)?.[1] };
  });
  const main = bins.find((bin) => bin.source === 'src/main.rs') ?? bins.find((bin) => bin.name === defaultRun);
  const name = main?.name ?? defaultRun ?? packageName;
  if (!packageName || !name) throw new Error('Could not identify the native client package and binary from Cargo.toml');
  return { package: packageName, binary: name };
}
export function clientArtifact(messages, sourceFile) {
  const source = realpathSync.native(sourceFile);
  const matches = messages.filter((message) => message.reason === 'compiler-artifact' && message.target?.kind?.includes('bin')
    && message.executable && !message.profile?.test && realpathSync.native(message.target.src_path) === source);
  if (matches.length !== 1) throw new Error(`Expected one native client artifact from ${sourceFile}; received ${matches.length}`);
  return matches[0];
}
export function clientEnvironmentContract(runSource) {
  const instance = runSource.match(/std::env::var_os\("([A-Z][A-Z0-9_]*INSTANCE)"\)/)?.[1];
  const port = runSource.match(/std::env::var_os\("([A-Z][A-Z0-9_]*API_PORT)"\)/)?.[1];
  if (!instance || !port) throw new Error('Could not read the native client instance and port environment contract');
  return { instance, port };
}
export function pairPlatformSupport(platform) {
  return platform === 'win32'
    ? { supported: true, method: 'WinEvent close/reopen hooks and UI Automation tray-count evidence' }
    : { supported: false, reason: `No native window event and tray driver is implemented for ${platform}.` };
}
export function assertDisposableHome(home, realHome = os.homedir(), tempRoot = os.tmpdir()) {
  const requested = path.resolve(home), ownerPath = path.resolve(realHome);
  const owner = existsSync(ownerPath) ? realpathSync.native(ownerPath) : ownerPath;
  const temp = existsSync(tempRoot) ? realpathSync.native(tempRoot) : path.resolve(tempRoot);
  const actual = existsSync(requested) ? realpathSync.native(requested) : requested;
  const protectedDirectories = [path.join(owner, `.${LEGACY}`), path.join(owner, `.${LEGACY}-dev`)];
  if (actual === owner || !withinDirectory(temp, actual)
    || protectedDirectories.some((directory) => actual === directory || withinDirectory(directory, actual))) {
    throw new Error('Refused a process-probe home outside the disposable temp root');
  }
  return actual;
}
export function attachRequest(ready, overrides = {}) {
  return { protocol: ready.protocol, api_version: ready.api_version, build: ready.build, identity: ready.identity, challenge: randomUUID(), operation: 'inspect', expected: { process: ready.process, epoch: ready.epoch }, ...overrides };
}
function broker(executable, command, request, { hold = false, env } = {}) {
  if (!env?.HOME || !env?.USERPROFILE) throw new Error('Probe child is missing its disposable home');
  const child = spawn(executable, [command, ...(hold ? ['--hold'] : [])], { env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  const exit = once(child, 'exit', { signal: deadline() });
  let error = '';
  child.stderr.on('data', bytes => { error = (error + bytes.toString()).slice(-4096); });
  let pending = '', rows = [], receivers = [];
  child.stdout.on('data', bytes => {
    pending += bytes.toString('utf8');
    if (pending.length > 32768) { child.kill(); return; }
    for (let at; (at = pending.indexOf('\n')) >= 0;) {
      const line = pending.slice(0, at); pending = pending.slice(at + 1);
      let value;
      try { value = JSON.parse(line); } catch { value = { invalid_frame: true }; }
      if (receivers.length) receivers.shift()(value); else rows.push(value);
    }
  });
  child.stdin.write(`${JSON.stringify(request)}\n`);
  if (!hold) child.stdin.end();
  const line = async () => {
    if (rows.length) return rows.shift();
    const signal = deadline();
    return Promise.race([
      new Promise(resolve => receivers.push(resolve)),
      exit.then(([code]) => { throw new Error(`Broker exited before framed acknowledgement (exit ${code}): ${error.trim() || 'no stderr'}`); }),
      new Promise((_, reject) => signal.addEventListener('abort', () => reject(new Error('Broker acknowledgement deadline')), { once: true })),
    ]);
  };
  return { child, exit, line, error: () => error };
}
async function freePort() {
  for (let attempt = 0; attempt < 16; attempt += 1) {
    const server = net.createServer(); server.listen(0, '127.0.0.1'); await once(server, 'listening');
    const port = server.address().port;
    await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    if (port === 7685 || port === 7686) continue;
    return assertSafePort(port);
  }
  throw new Error('Could not select a process-probe port outside protected ports');
}
async function api(reply, endpoint, body) {
  const response = await fetch(`http://${reply.ready.api_address}${endpoint}`, { method: body ? 'POST' : 'GET', headers: { Authorization: `Bearer ${reply.api_token}`, 'Content-Type': 'application/json' }, ...(body ? { body: JSON.stringify(body) } : {}), signal: deadline() });
  assert.equal(response.ok, true, `Isolated API refused ${endpoint}: ${response.status}`);
  return response.json();
}
const WINDOWS_PAIR_SCRIPT = String.raw`
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
$uiAutomationReferences = @([System.Windows.Automation.AutomationElement].Assembly.Location,
  [System.Windows.Automation.TreeScope].Assembly.Location)
Add-Type -ReferencedAssemblies $uiAutomationReferences -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Automation;
namespace PairProbe {
  public static class NativeWindow {
    private delegate bool EnumWindowsDelegate(IntPtr hwnd, IntPtr state);
    private delegate void WinEventDelegate(IntPtr hook, uint eventType, IntPtr hwnd, int objectId, int childId, uint threadId, uint time);
    [StructLayout(LayoutKind.Sequential)] private struct Point { public int X; public int Y; }
    [StructLayout(LayoutKind.Sequential)] private struct Message { public IntPtr Hwnd; public uint Id; public IntPtr WParam; public IntPtr LParam; public uint Time; public Point Point; }
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsDelegate callback, IntPtr state);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("user32.dll")] private static extern IntPtr SetWinEventHook(uint eventMin, uint eventMax, IntPtr module, WinEventDelegate callback, uint processId, uint threadId, uint flags);
    [DllImport("user32.dll")] private static extern bool UnhookWinEvent(IntPtr hook);
    [DllImport("user32.dll")] private static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] private static extern uint MsgWaitForMultipleObjects(uint count, IntPtr[] handles, bool waitAll, uint milliseconds, uint wakeMask);
    [DllImport("user32.dll")] private static extern bool PeekMessage(out Message message, IntPtr hwnd, uint min, uint max, uint remove);
    [DllImport("user32.dll")] private static extern bool TranslateMessage(ref Message message);
    [DllImport("user32.dll")] private static extern IntPtr DispatchMessage(ref Message message);
    private const uint Create = 0x8000, Destroy = 0x8001, Show = 0x8002, Hide = 0x8003;
    private const uint OutOfContext = 0x0000, SkipOwnProcess = 0x0002, Remove = 0x0001, AllInput = 0x04FF, CloseMessage = 0x0010;
    private const uint WaitObject0 = 0, WaitMessage = 0x000000C0;
    public static int TrayCount(string trayName) {
      Condition condition = new PropertyCondition(AutomationElement.NameProperty, trayName);
      return AutomationElement.RootElement.FindAll(TreeScope.Descendants, condition).Count;
    }
    public static int WaitTrayCount(string trayName, int expected, int timeoutMilliseconds) {
      using (ManualResetEvent signal = new ManualResetEvent(false)) {
        StructureChangedEventHandler callback = (sender, args) => { if (TrayCount(trayName) >= expected) signal.Set(); };
        AutomationElement root = AutomationElement.RootElement;
        Automation.AddStructureChangedEventHandler(root, TreeScope.Descendants, callback);
        try {
          if (TrayCount(trayName) < expected && !signal.WaitOne(timeoutMilliseconds)) throw new TimeoutException("Native client tray icon did not appear before the deadline.");
          int count = TrayCount(trayName);
          if (count != expected) throw new InvalidOperationException("Native client tray icon count differs from the expected isolated count.");
          return count;
        } finally { Automation.RemoveStructureChangedEventHandler(root, callback); }
      }
    }
    public static IntPtr FindWindow(uint wantedPid) {
      IntPtr found = IntPtr.Zero;
      EnumWindows((hwnd, state) => { uint pid; GetWindowThreadProcessId(hwnd, out pid); if (pid == wantedPid && IsWindowVisible(hwnd)) { found = hwnd; return false; } return true; }, IntPtr.Zero);
      return found;
    }
    private static bool HasVisibleWindow(uint wantedPid) { return FindWindow(wantedPid) != IntPtr.Zero; }
    private static bool WaitForState(uint wantedPid, bool visible, IntPtr closeWindow, int timeoutMilliseconds) {
      using (ManualResetEvent signal = new ManualResetEvent(false)) {
        IntPtr handle = signal.SafeWaitHandle.DangerousGetHandle();
        WinEventDelegate callback = (hook, eventType, hwnd, objectId, childId, threadId, time) => {
          uint pid; GetWindowThreadProcessId(hwnd, out pid);
          bool stateEvent = visible ? (eventType == Create || eventType == Show) : (eventType == Destroy || eventType == Hide);
          bool ownsWindow = !visible && closeWindow != IntPtr.Zero ? hwnd == closeWindow : pid == wantedPid;
          if (ownsWindow && objectId == 0 && childId == 0 && stateEvent && IsWindowVisible(hwnd) == visible) signal.Set();
        };
        IntPtr hook = SetWinEventHook(Create, Hide, IntPtr.Zero, callback, 0, 0, OutOfContext | SkipOwnProcess);
        if (hook == IntPtr.Zero) throw new InvalidOperationException("SetWinEventHook failed");
        try {
          if (visible == HasVisibleWindow(wantedPid)) return true;
          if (closeWindow != IntPtr.Zero && !PostMessage(closeWindow, CloseMessage, IntPtr.Zero, IntPtr.Zero)) return false;
          Stopwatch watch = Stopwatch.StartNew();
          while (!signal.WaitOne(0)) {
            long remaining = timeoutMilliseconds - watch.ElapsedMilliseconds;
            if (remaining <= 0) return false;
            IntPtr[] handles = new IntPtr[] { handle };
            uint result = MsgWaitForMultipleObjects(1, handles, false, (uint)remaining, AllInput);
            if (result == WaitObject0) break;
            if (result == WaitMessage) {
              Message message;
              while (PeekMessage(out message, IntPtr.Zero, 0, 0, Remove)) { TranslateMessage(ref message); DispatchMessage(ref message); }
            }
          }
          return visible == HasVisibleWindow(wantedPid);
        } finally { UnhookWinEvent(hook); GC.KeepAlive(callback); }
      }
    }
    public static bool CloseAndWait(uint pid, int timeoutMilliseconds) {
      IntPtr hwnd = FindWindow(pid);
      if (hwnd == IntPtr.Zero) return false;
      return WaitForState(pid, false, hwnd, timeoutMilliseconds);
    }
    public static bool WaitVisible(uint pid, int timeoutMilliseconds) { return WaitForState(pid, true, IntPtr.Zero, timeoutMilliseconds); }
  }
}
'@
$pidValue = [uint32]::Parse($env:PAIR_PROBE_PID)
$timeout = [int]::Parse($env:PAIR_PROBE_TIMEOUT)
switch ($env:PAIR_PROBE_ACTION) {
  'close' {
    $ok = [PairProbe.NativeWindow]::CloseAndWait($pidValue, $timeout)
    if (-not $ok) { throw 'Native client window did not close before the deadline.' }
  }
  'wait-visible' {
    $ok = [PairProbe.NativeWindow]::WaitVisible($pidValue, $timeout)
    if (-not $ok) { throw 'Native client window did not reopen before the deadline.' }
  }
  'wait-tray-count' {
    $expected = [int]::Parse($env:PAIR_PROBE_TRAY_COUNT)
    [void][PairProbe.NativeWindow]::WaitTrayCount($env:PAIR_PROBE_TRAY_NAME, $expected, $timeout)
  }
  'count-tray' { }
  default { throw 'Unknown pair-probe window action.' }
}
$matches = [PairProbe.NativeWindow]::TrayCount($env:PAIR_PROBE_TRAY_NAME)
if ($env:PAIR_PROBE_ACTION -eq 'count-tray' -and $matches -lt 0) { throw 'Could not inspect Windows tray entries.' }
if ($env:PAIR_PROBE_ACTION -ne 'count-tray' -and $matches -lt 1) { throw 'Native client tray icon was not present in UI Automation.' }
$window = [PairProbe.NativeWindow]::FindWindow($pidValue)
Write-Output (@{ status = 'verified'; visible = ($window -ne [IntPtr]::Zero); tray = $env:PAIR_PROBE_TRAY_NAME; tray_count = $matches; window_handle = $window.ToInt64() } | ConvertTo-Json -Compress)
`;

export function windowsPairAction(action, pid, { timeoutMs = 10_000, trayName, trayCount = null }) {
  if (process.platform !== 'win32') throw new Error('The native window driver is available only on Windows');
  const inherited = { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, WINDIR: process.env.WINDIR, PSModulePath: process.env.PSModulePath };
  const command = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', WINDOWS_PAIR_SCRIPT], {
    encoding: 'utf8', timeout: timeoutMs + 2_000, windowsHide: true,
    env: { ...Object.fromEntries(Object.entries(inherited).filter(([, value]) => value !== undefined)),
      PAIR_PROBE_ACTION: action, PAIR_PROBE_PID: String(pid), PAIR_PROBE_TIMEOUT: String(timeoutMs),
      PAIR_PROBE_TRAY_NAME: trayName, PAIR_PROBE_TRAY_COUNT: trayCount === null ? '' : String(trayCount) },
  });
  if (command.error) throw new Error(`Windows UI driver failed: ${command.error.message}`);
  if (command.status !== 0) throw new Error(`Windows UI driver refused ${action}: ${(command.stderr || command.stdout).slice(-1200)}`);
  const rows = command.stdout.trim().split(/\r?\n/);
  const result = JSON.parse(rows.at(-1));
  assert.equal(result.status, 'verified');
  assert.equal(result.tray, trayName);
  return result;
}

function treeSnapshot(root) {
  const rows = [];
  const visit = (directory) => {
    for (const name of readdirSync(directory, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
      const absolute = path.join(directory, name.name), relative = path.relative(root, absolute).replaceAll('\\', '/');
      rows.push(relative);
      if (name.isDirectory()) visit(absolute);
    }
  };
  visit(root);
  return rows;
}

function waitForLine(child, prefix, timeoutMs = 60_000) {
  return new Promise((resolve, reject) => {
    let pending = '', size = 0, settled = false;
    const signal = AbortSignal.timeout(timeoutMs);
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener('abort', onAbort);
      error ? reject(error) : resolve(value);
    };
    const onAbort = () => finish(new Error(`Native client attachment deadline (${timeoutMs}ms)`));
    signal.addEventListener('abort', onAbort, { once: true });
    const consume = (bytes) => {
      size += bytes.length;
      if (size > 65536) { finish(new Error('Native client output exceeded the observation bound')); return; }
      pending += bytes.toString('utf8');
      for (let end; (end = pending.indexOf('\n')) >= 0;) {
        const line = pending.slice(0, end).replace(/\r$/, '');
        pending = pending.slice(end + 1);
        if (!line.startsWith(`${prefix} `)) continue;
        try { finish(null, JSON.parse(line.slice(prefix.length + 1))); }
        catch { finish(new Error('Native client attachment line was not JSON')); }
      }
    };
    child.stdout.on('data', consume);
    child.stderr.on('data', consume);
    child.once('error', (error) => finish(error));
    child.once('exit', (code, exitSignal) => finish(new Error(`Native client exited before attachment (code=${code}, signal=${exitSignal})`)));
  });
}

function spawnNativeClient(executable, env, { readinessPrefix = null } = {}) {
  const child = spawn(executable, [], { cwd: REPO, env, windowsHide: false, stdio: ['ignore', 'pipe', 'pipe'] });
  const ready = readinessPrefix ? waitForLine(child, readinessPrefix) : null;
  const exit = new Promise((resolve) => {
    child.once('error', (error) => resolve({ error }));
    child.once('exit', (code, signal) => resolve({ code, signal }));
  });
  return { child, ready, exit };
}

async function clientExec(reply, tabId, command) {
  const result = await api(reply, `/api/tabs/${tabId}/exec`, { command, timeout_ms: 15_000, strip_ansi: true });
  assert.equal(result.completion, 'command-finished', `PTY command did not complete: ${JSON.stringify(result)}`);
  assert.equal(result.exit_code, 0, `PTY command failed: ${JSON.stringify(result)}`);
  return result;
}

function clientContract() {
  const runSource = readFileSync(path.join(REPO, 'src-tauri/src/run.rs'), 'utf8');
  const windowSource = readFileSync(path.join(REPO, 'src-tauri/src/startup/window.rs'), 'utf8');
  const contract = clientEnvironmentContract(runSource);
  const readiness = runSource.match(/eprintln!\("([A-Z][A-Z0-9_]+) \{readiness\}"\)/)?.[1];
  const tray = windowSource.match(/\.tooltip\("([^"]+)"\)/)?.[1];
  if (!readiness || !tray) throw new Error('Native client readiness or tray contract is absent');
  return { ...contract, readiness, tray };
}

function assertClientReceipt(receipt, serverReceipt) {
  assert.equal(receipt.profile, 'dev');
  assert.equal(receipt.target, serverReceipt.target);
  assert.ok(receipt.binary_name && path.basename(receipt.executable).replace(/\.exe$/i, '') === receipt.binary_name);
  assert.ok(existsSync(receipt.executable), `Native client artifact is missing: ${receipt.executable}`);
  assert.equal(createHash('sha256').update(readFileSync(receipt.executable)).digest('hex'), receipt.executable_sha256);
}

function waitChildExit(client, timeoutMs = 10_000) {
  if (client.child.exitCode !== null) return Promise.resolve({ code: client.child.exitCode, signal: client.child.signalCode });
  return Promise.race([
    client.exit,
    new Promise((_, reject) => {
      const signal = AbortSignal.timeout(timeoutMs);
      signal.addEventListener('abort', () => reject(new Error(`Client process ${client.child.pid} did not exit before ${timeoutMs}ms`)), { once: true });
    }),
  ]).then((result) => {
    if (result.error) throw new Error(`Native client process ${client.child.pid ?? '<unspawned>'} failed: ${result.error.message}`);
    return result;
  });
}

async function pairJourney({ options, receipt, reply, env, scratch, home, instance, tab, originalPtyId, nativeClients }) {
  const rows = [];
  const health = await fetch(`http://${reply.ready.api_address}/api/health`, { signal: deadline() }).then((response) => response.json());
  assert.equal(health.status, 'ok');
  assert.equal(health.build_sha, receipt.build, 'Server-alone health must identify the probed build');
  assert.equal(health.instance?.name, instance, 'Server-alone health must identify the disposable instance');
  assert.equal(health.instance?.api_port, reply.ready.process.api_port ?? Number(reply.ready.api_address.split(':').at(-1)));
  assert.equal(nativeClients.length, 0, 'No native client may start before the server-alone row');
  rows.push({ name: 'server_alone', status: 'PASS', evidence: { server_pid: reply.ready.process.pid, api_address: reply.ready.api_address, client_started: false } });

  const before = `pair-before-${randomUUID()}`;
  const firstOutput = await clientExec(reply, tab.id, `echo ${before}`);
  assert.ok(firstOutput.output.includes(before), 'PTY output was not delivered before client attach');
  const platform = pairPlatformSupport(process.platform);
  if (!platform.supported) {
    rows.push({ name: 'native_attach_close_reopen_restart', status: 'UNVERIFIED', reason: platform.reason });
    return { status: 'UNVERIFIED', rows, pty_id: originalPtyId, server_pid: reply.ready.process.pid };
  }

  const client = JSON.parse(readFileSync(options.clientReceipt, 'utf8'));
  assertClientReceipt(client, receipt);
  const contract = clientContract();
  const trayBefore = windowsPairAction('count-tray', 0, { trayName: contract.tray }).tray_count;
  const nativeEnv = cleanEnvironment(process.env, home);
  nativeEnv[contract.instance] = instance;
  nativeEnv[contract.port] = String(assertSafePort(Number(reply.ready.api_address.split(':').at(-1))));
  const initial = spawnNativeClient(client.executable, nativeEnv, { readinessPrefix: contract.readiness });
  nativeClients.push(initial);
  const attached = await initial.ready;
  const trayAttached = windowsPairAction('wait-tray-count', initial.child.pid,
    { trayName: contract.tray, trayCount: trayBefore + 1 });
  assert.equal(attached.client_pid, initial.child.pid, 'Attachment evidence must identify the launched native client');
  assert.equal(attached.server_pid, reply.ready.process.pid, 'Native client attached to a different server process');
  assert.equal(attached.epoch, reply.ready.epoch, 'Native client attached to a different server epoch');
  rows.push({ name: 'native_client_attach', status: 'PASS', evidence: { client_pid: attached.client_pid, server_pid: attached.server_pid,
    epoch: attached.epoch, tray_count_before: trayBefore, tray_count_after: trayAttached.tray_count } });

  const closed = windowsPairAction('close', initial.child.pid, { trayName: contract.tray });
  assert.equal(closed.visible, false, 'Close must hide or destroy the native window');
  assert.equal(initial.child.exitCode, null, 'Close must retain the native client process in the tray');
  assert.equal(closed.tray_count, trayAttached.tray_count, 'Closing the window removed the isolated tray entry');
  const afterClose = await inspect(receipt.executable, reply, undefined, env);
  assert.equal(afterClose.ready.process.pid, reply.ready.process.pid, 'Closing the window changed the server PID');
  const tabsAfterClose = await api(afterClose, '/api/tabs?local_only=true');
  assert.equal(tabsAfterClose.find((row) => row.id === tab.id)?.pty_id, originalPtyId, 'Closing the window changed the PTY session');
  const closeOutput = await clientExec(reply, tab.id, `echo pair-close-${randomUUID()}`);
  assert.ok(closeOutput.offset > firstOutput.offset, 'PTY output offset did not continue after window close');
  rows.push({ name: 'window_close_tray_retained', status: 'PASS', evidence: { client_pid: initial.child.pid, server_pid: afterClose.ready.process.pid,
    pty_id: originalPtyId, window_visible: closed.visible, tray_count: closed.tray_count, output_offset: closeOutput.offset } });

  const activation = spawnNativeClient(client.executable, nativeEnv);
  nativeClients.push(activation);
  const reopened = windowsPairAction('wait-visible', initial.child.pid, { trayName: contract.tray });
  const activationExit = await waitChildExit(activation, 15_000);
  assert.equal(activationExit.code, 0, 'Reopen activation process did not hand off cleanly');
  assert.equal(reopened.visible, true, 'Native window did not reopen');
  assert.equal(reopened.tray_count, trayAttached.tray_count, 'Reopening changed the isolated tray entry count');
  const afterReopen = await inspect(receipt.executable, reply, undefined, env);
  assert.equal(afterReopen.ready.process.pid, reply.ready.process.pid, 'Reopening the window changed the server PID');
  const tabsAfterReopen = await api(afterReopen, '/api/tabs?local_only=true');
  assert.equal(tabsAfterReopen.find((row) => row.id === tab.id)?.pty_id, originalPtyId);
  rows.push({ name: 'window_reopen', status: 'PASS', evidence: { client_pid: initial.child.pid, server_pid: afterReopen.ready.process.pid,
    pty_id: originalPtyId, window_visible: reopened.visible, activation_exit: activationExit.code } });

  initial.child.kill();
  const clientExit = await waitChildExit(initial);
  assert.ok(clientExit.code !== null || clientExit.signal !== null, 'Client-only restart must terminate only the original client process');
  const noClient = await inspect(receipt.executable, reply, undefined, env);
  assert.equal(noClient.ready.process.pid, reply.ready.process.pid, 'Client exit changed the server PID');
  const duringRestart = await clientExec(reply, tab.id, `echo pair-no-client-${randomUUID()}`);
  assert.ok(duringRestart.offset > closeOutput.offset, 'Server PTY stopped producing output while the client was down');
  const restarted = spawnNativeClient(client.executable, nativeEnv, { readinessPrefix: contract.readiness });
  nativeClients.push(restarted);
  const restartedReady = await restarted.ready;
  assert.notEqual(restartedReady.client_pid, attached.client_pid, 'Client restart reused the terminated process');
  assert.equal(restartedReady.server_pid, reply.ready.process.pid, 'Restarted client attached to a different server process');
  assert.equal(restartedReady.epoch, reply.ready.epoch, 'Restarted client attached to a different server epoch');
  const afterRestart = await inspect(receipt.executable, reply, undefined, env);
  const tabsAfterRestart = await api(afterRestart, '/api/tabs?local_only=true');
  assert.equal(tabsAfterRestart.find((row) => row.id === tab.id)?.pty_id, originalPtyId, 'Client-only restart changed the PTY session');
  const after = `pair-after-${randomUUID()}`;
  const finalOutput = await clientExec(reply, tab.id, `echo ${after}`);
  assert.ok(finalOutput.output.includes(after), 'PTY output was not delivered after native client restart');
  assert.ok(finalOutput.offset > duringRestart.offset, 'PTY output offset did not continue across client restart');
  rows.push({ name: 'client_only_restart', status: 'PASS', evidence: { old_client_pid: attached.client_pid, new_client_pid: restartedReady.client_pid,
    server_pid: afterRestart.ready.process.pid, epoch: restartedReady.epoch, pty_id: originalPtyId,
    output_offsets: [firstOutput.offset, closeOutput.offset, duringRestart.offset, finalOutput.offset], output_marker: after } });
  restarted.child.kill();
  const restartedExit = await waitChildExit(restarted);
  if (restartedExit.code === null && restartedExit.signal === null) throw new Error(`Native client cleanup did not exit: ${JSON.stringify(restartedExit)}`);
  return { status: 'PASS', rows, server_pid: reply.ready.process.pid, pty_id: originalPtyId, client_pids: [attached.client_pid, restartedReady.client_pid], scratch };
}
async function inspect(executable, reply, overrides, env) {
  const request = attachRequest(reply.ready, overrides);
  const helper = broker(executable, 'attach', request, { env });
  const result = await helper.line();
  assert.equal((await helper.exit)[0], 0);
  assert.equal(result.challenge, request.challenge);
  assert.deepEqual(result.ready, reply.ready);
  return result;
}
async function negative(executable, reply, overrides, env) {
  const helper = broker(executable, 'attach', attachRequest(reply.ready, overrides), { env });
  assert.notEqual((await helper.exit)[0], 0, 'Invalid attach must refuse');
}
function socketName(name) {
  if (process.platform === 'win32') return `\\\\.\\pipe\\${name}`;
  return process.platform === 'linux' ? `\0${name}` : `/tmp/${name}`;
}
export function socketProbeRequest(reply, protocolSource = readFileSync(path.join(REPO, 'cli/src/protocol.rs'), 'utf8')) {
  const version = Number(protocolSource.match(/const PROTOCOL_VERSION: u64 = (\d+);/)?.[1]);
  if (!Number.isSafeInteger(version) || version <= 0) throw new Error('Socket protocol contract is absent');
  if (typeof reply.api_token !== 'string' || !reply.api_token) throw new Error('Broker credential is absent');
  return { jsonrpc: '2.0', id: 1, method: 'system.identify', protocol_version: version, credential: reply.api_token };
}
async function socketProof(reply) {
  const ready = reply.ready;
  const request = socketProbeRequest(reply);
  const name = ready.socket_address.split(/[\\/]/).at(-1);
  const socket = net.createConnection(socketName(name));
  let pending = '';
  const signal = deadline();
  const data = new Promise((resolve, reject) => {
    socket.on('data', bytes => {
      pending += bytes.toString('utf8');
      if (pending.length > 32768) reject(new Error('Socket acknowledgement exceeds bound'));
      if (pending.includes('\n')) { try { resolve(JSON.parse(pending.slice(0, pending.indexOf('\n')))); } catch { reject(new Error('Invalid socket acknowledgement')); } }
    });
    socket.once('error', reject);
    socket.once('end', () => reject(new Error('Socket ended before acknowledgement')));
    signal.addEventListener('abort', () => reject(new Error('Socket acknowledgement deadline')), { once: true });
  });
  await once(socket, 'connect', { signal: deadline() });
  socket.write(`${JSON.stringify(request)}\n`);
  try { assert.ok((await data).result?.version); }
  finally { socket.destroy(); }
}
async function maintenanceEvent(data, receipt) {
  const file = path.join(data, 'process-worker.jsonl');
  const signal = deadline();
  let watcher;
  try {
    const event = await new Promise((resolve, reject) => {
      const read = () => {
        try {
          const content = readFileSync(file, 'utf8');
          if (content.length > 32768) throw new Error('Worker observation exceeds bound');
          if (content.includes('\n')) resolve(JSON.parse(content.slice(0, content.indexOf('\n'))));
        } catch (error) { reject(error); }
      };
      watcher = watch(data, (_, name) => { if (name?.toString() === 'process-worker.jsonl') read(); });
      watcher.once('error', reject);
      signal.addEventListener('abort', () => reject(new Error('Actual maintenance observation deadline')), { once: true });
      read();
    });
    assert.equal(event.status, 'verified', 'Actual maintenance child provenance unavailable; native row fails');
    assert.equal(event.resource_sha256, receipt.worker_cli_sha256);
    assert.equal(event.selected, event.process.executable);
    assert.ok(event.process.pid > 0 && event.process.created);
    return event;
  } finally { watcher?.close(); }
}
async function foreignExecutableRefuses(ready) {
  const socket = net.createConnection(socketName(ready.bootstrap_address));
  const closed = once(socket, 'close', { signal: deadline() });
  let received = 0;
  socket.on('data', bytes => { received += bytes.length; });
  socket.on('error', () => {});
  await closed;
  assert.equal(received, 0, 'Foreign executable must not receive a bootstrap credential');
}
async function run(options) {
  if (!options.receipt) throw new Error('Execution requires --artifact-receipt from the isolated build');
  if (options.case === 'pair-journey' && !options.clientReceipt) throw new Error('Pair journey requires the exact native client artifact receipt');
  const receipt = JSON.parse(readFileSync(options.receipt, 'utf8'));
  assertServerClosure(receipt.dependency_packages);
  assert.deepEqual(receipt.features, SERVER_FEATURES);
  assert.equal(receipt.profile, 'dev');
  assert.equal(createHash('sha256').update(readFileSync(receipt.executable)).digest('hex'), receipt.executable_sha256);
  const instance = `split-oracle-${randomUUID().slice(0, 8)}`;
  const scratch = mkdtempSync(path.join(os.tmpdir(), 'domios-process-probe-'));
  const home = assertDisposableHome(path.join(scratch, 'isolated-home'));
  const env = cleanEnvironment(process.env, home);
  const port = await freePort();
  const launch = { instance: { kind: 'development', instance, port }, resources: receipt.resources, probe_sink: path.join(scratch, 'process-init.jsonl') };
  const launchers = []; const nativeClients = []; let reply; let observation; let stopped = false; let result; let alias = null; let tab = null; let data = null;
  let failure = null;
  try {
    const homeBefore = treeSnapshot(home);
    const rejectedInstance = `${instance}-missing`;
    const rejectedSink = path.join(scratch, 'rejected-init.jsonl');
    const missing = broker(receipt.executable, 'launch', { ...launch, resources: scratch, instance: { ...launch.instance, instance: rejectedInstance }, probe_sink: rejectedSink }, { env });
    assert.notEqual((await missing.exit)[0], 0, 'Missing exact CLI resource must refuse before service initialization');
    assert.equal(existsSync(rejectedSink), false, 'Rejected launch must not write its initialization probe');
    assert.deepEqual(treeSnapshot(home), homeBefore, 'Rejected launch modified the disposable home before initialization');
    const first = broker(receipt.executable, 'launch', launch, { hold: true, env }); launchers.push(first);
    reply = await first.line();
    assert.equal(reply.ready.build, receipt.build);
    assert.equal(realpathSync.native(reply.ready.process.executable), realpathSync.native(receipt.executable));
    data = realpathSync.native(reply.ready.identity.data);
    assert.ok(withinDirectory(home, data), 'Server data directory escaped the disposable home');
    assert.notEqual(data, realpathSync.native(os.homedir()), 'Server data directory resolved to the owner home');
    assert.notEqual(reply.ready.process.pid, process.pid);
    assert.equal(reply.ready.api_version, 3);
    assert.equal(reply.ready.api_address, `127.0.0.1:${assertSafePort(port)}`);
    assert.ok(withinDirectory(scratch, launch.probe_sink));
    if (options.case === 'arbitration') {
      alias = path.join(scratch, 'home-alias');
      symlinkSync(home, alias, process.platform === 'win32' ? 'junction' : 'dir');
      const aliasData = path.join(alias, path.relative(home, data));
      const challenger = broker(receipt.executable, 'launch', { ...launch,
        probe_identity_alias: { data: aliasData, config: path.join(aliasData, 'config.toml') } }, { hold: true, env });
      launchers.push(challenger);
      assert.notEqual((await challenger.exit)[0], 0, 'An alias to the live isolated instance must not claim ownership');
    }
    const auditBefore = readFileSync(launch.probe_sink, 'utf8');
    const audit = auditBefore.trim().split('\n').map(JSON.parse);
    assert.equal(new Set(audit.map(row => row.pid)).size, 1);
    assert.deepEqual(audit.map(row => row.phase), ['mesh', 'crash', 'config-credential-hook-state']);
    assert.equal(audit[0].pid, reply.ready.process.pid);
    const maintenance = await maintenanceEvent(data, receipt);
    assert.notEqual(maintenance.process.pid, reply.ready.process.pid);
    const observationRequest = attachRequest(reply.ready);
    observation = broker(receipt.executable, 'observe', observationRequest, { env });
    assert.deepEqual((await observation.line()).observing, reply.ready);
    const workspace = await api(reply, '/api/workspaces', { name: 'ProcessProbe', cwd: scratch });
    tab = await api(reply, '/api/tabs', { workspace_id: workspace.id, cwd: scratch, title: 'Probe', eager: true });
    assert.ok(tab.pty_id, 'Disposable session must own a real PTY before launcher termination');
    let pair;
    if (options.case === 'pair-journey') {
      pair = await pairJourney({ options, receipt, reply, env, scratch, home, instance, tab, originalPtyId: tab.pty_id, nativeClients });
    }
    for (const helper of launchers) {
      if (helper.child.exitCode === null) helper.child.kill();
      await helper.exit;
    }
    const reattached = await inspect(receipt.executable, reply, undefined, env);
    await socketProof(reply);
    await foreignExecutableRefuses(reply.ready);
    const tabs = await api(reattached, '/api/tabs?local_only=true');
    const same = tabs.find(row => row.id === tab.id);
    assert.ok(same, 'Session must survive launcher exit');
    assert.equal(same.pty_id, tab.pty_id);
    if (options.case === 'arbitration') {
      await negative(receipt.executable, reply, { protocol: reply.ready.protocol + 1 }, env);
      await negative(receipt.executable, reply, { build: `${reply.ready.build}-foreign` }, env);
      await negative(receipt.executable, reply, { expected: { process: reply.ready.process, epoch: 'stale-epoch' } }, env);
      await negative(receipt.executable, reply, { expected: { process: { ...reply.ready.process, created: 'reused-pid' }, epoch: reply.ready.epoch } }, env);
      await negative(receipt.executable, reply, { identity: { ...reply.ready.identity, data: path.join(scratch, 'foreign') } }, env);
      assert.equal(readFileSync(launch.probe_sink, 'utf8'), auditBefore, 'Rejected contenders must not initialize again');
      const occupiedPort = await freePort();
      const occupied = net.createServer(); occupied.listen(occupiedPort, '127.0.0.1'); await once(occupied, 'listening');
      const foreignInstance = `${instance}-occupied`;
      const foreignSink = path.join(scratch, 'foreign-init.jsonl');
      try {
        const conflict = broker(receipt.executable, 'launch', { ...launch, instance: { kind: 'development', instance: foreignInstance, port: assertSafePort(occupied.address().port) }, probe_sink: foreignSink }, { env });
        assert.notEqual((await conflict.exit)[0], 0, 'Foreign listener cannot be claimed as ready');
        assert.equal(occupied.listening, true, 'Foreign listener must retain its own OS handle');
        assert.equal(existsSync(foreignSink), false, 'Conflicting launch must not initialize the foreign instance');
      } finally { await new Promise(resolve => occupied.close(resolve)); }
      const forbidden = broker(receipt.executable, 'launch', { ...launch, instance: { kind: 'production' } }, { env });
      assert.notEqual((await forbidden.exit)[0], 0, 'DEV artifact must refuse PROD selection');
    }
    await inspect(receipt.executable, reply, { operation: 'shutdown' }, env);
    assert.deepEqual(await observation.line(), { exited: true });
    assert.equal((await observation.exit)[0], 0);
    stopped = true;
    result = { status: pair?.status ?? 'PASS', case: options.case, platform: process.platform, target: receipt.target, build: receipt.build,
      ready: reply.ready, maintenance, ...(alias ? { alias: { source: alias, target: realpathSync.native(alias), kind: process.platform === 'win32' ? 'junction' : 'symlink' } } : {}),
      missing_resource_refused_before_initialization: true, initialization_phases: audit.length, frontend_acknowledgements: 0,
      exact_exit_observed: true, ...(pair ? { rows: pair.rows, server_pid: pair.server_pid, pty_id: pair.pty_id, client_pids: pair.client_pids } : {}) };
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    if (reply && !stopped) {
      try { await inspect(receipt.executable, reply, { operation: 'shutdown' }, env); if (observation) { await observation.line(); await observation.exit; } stopped = true; } catch { /* Retain data if exact cleanup cannot be proven. */ }
    }
    for (const client of nativeClients) {
      if (client.child.exitCode === null) {
        client.child.kill();
        await waitChildExit(client, 10_000);
      }
    }
    for (const helper of launchers) if (helper.child.exitCode === null) { helper.child.kill(); await helper.exit; }
    if (stopped) rmSync(scratch, { recursive: true });
    if (!stopped) throw new Error(`Exact server cleanup unproven; disposable data retained at ${scratch}`, { cause: failure ?? undefined });
  }
  console.log(JSON.stringify(result));
  return result;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === '--validate') {
    parseProbeArgs(process.argv.slice(3));
  } else if (process.argv[2] === '--client-build-target') {
    const target = clientBuildTarget(readFileSync(path.join(REPO, 'src-tauri/Cargo.toml'), 'utf8'));
    process.stdout.write(`${target.package}\t${target.binary}\n`);
  } else if (process.argv[2] === '--write-client-receipt') {
    const [log, output, build, target] = process.argv.slice(3);
    const messages = readFileSync(log, 'utf8').split(/\r?\n/).filter(Boolean).map((line) => JSON.parse(line));
    if (!messages.some((message) => message.reason === 'build-finished' && message.success)) throw new Error('Native client Cargo build did not finish successfully');
    const artifact = clientArtifact(messages, path.join(REPO, 'src-tauri/src/main.rs'));
    const executable = realpathSync.native(artifact.executable);
    const receipt = { executable, executable_sha256: createHash('sha256').update(readFileSync(executable)).digest('hex'),
      package_id: artifact.package_id, binary_name: artifact.target.name, source: realpathSync.native(artifact.target.src_path),
      build, target, profile: 'dev' };
    writeFileSync(output, `${JSON.stringify(receipt, null, 2)}\n`);
  } else run(parseProbeArgs(process.argv.slice(2))).then((result) => {
    process.exitCode = result.status === 'PASS' ? 0 : 1;
  }, (error) => {
    console.error(`process-probe FAILED: ${error.message}`);
    if (error.cause) console.error(`  caused by: ${error.cause.stack ?? error.cause}`);
    process.exitCode = 1;
  });
}
