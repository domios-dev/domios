import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { bashInterpreter } from './bash-interpreter.mjs';
import { ownerState, readOwner } from './build-gateway-owner.mjs';

const [operation, value, ...args] = process.argv.slice(2);
const windows = process.platform === 'win32';

async function cancel(directory) {
  const owner = readOwner(directory);
  if (ownerState(owner) !== 'alive') throw new Error('Gateway owner identity is unavailable or stale');
  const endpoint = fs.readFileSync(path.join(directory, 'supervisor_endpoint'), 'utf8').trim();
  if (!endpoint) throw new Error('Gateway owner has no process supervisor; cancellation refused');
  const socket = net.createConnection(endpoint);
  socket.setEncoding('utf8');
  socket.setTimeout(30_000, () => socket.destroy(new Error('Gateway cancellation acknowledgement timed out')));
  const response = new Promise((resolve, reject) => {
    let body = '';
    socket.on('data', chunk => { body += chunk; });
    socket.on('end', () => body === 'stopped\n' ? resolve() : reject(new Error(body || 'Missing cancellation acknowledgement')));
    socket.on('error', reject);
  });
  socket.on('connect', () => socket.write(JSON.stringify(owner) + '\n'));
  await response;
}

async function run(script, commandArgs) {
  const endpoint = windows ? `\\\\.\\pipe\\domios-build-${randomUUID()}`
    : path.join(os.tmpdir(), `domios-build-${randomUUID()}.sock`);
  let child, exited, stopping;
  const stop = () => stopping ??= (async () => {
    if (!child?.pid || child.exitCode !== null || child.signalCode !== null) throw new Error('Gateway owner already exited');
    if (windows) {
      // Retain native handles before taskkill so success means every observed descendant has exited.
      const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `
        $ErrorActionPreference='Stop'; $root=${child.pid}; $ids=@($root); $handles=@();
        $rows=@(Get-CimInstance Win32_Process);
        for ($i=0; $i -lt $ids.Count; $i++) {
          $ids += @($rows | Where-Object { $_.ParentProcessId -eq $ids[$i] } | ForEach-Object { [int]$_.ProcessId });
        }
        foreach ($id in $ids) { try { $p=[Diagnostics.Process]::GetProcessById($id); $null=$p.Handle; $handles+=,$p } catch [ArgumentException] {} }
        & taskkill.exe /PID $root /T /F | Out-Null;
        if ($LASTEXITCODE -ne 0) { throw 'taskkill failed' }
        foreach ($p in $handles) { $p.WaitForExit(); $p.Dispose() }
      `], { encoding: 'utf8', timeout: 30_000 });
      if (result.status !== 0) throw new Error(result.stderr || result.error?.message || 'taskkill failed');
    } else {
      process.kill(-child.pid, 'SIGSTOP');
      const gateway = fileURLToPath(new URL('./build-gateway.sh', import.meta.url));
      const result = spawnSync(bashInterpreter(), ['-c', `source "$1"
        for descendant in $(pgrep -P "$2"); do ct_gateway_terminate_descendant_tree "$descendant"; done`,
      'gateway-stop', gateway, String(child.pid)], { encoding: 'utf8', timeout: 30_000 });
      if (result.status !== 0) throw new Error(result.stderr || 'Gateway descendant termination failed');
      process.kill(-child.pid, 'SIGKILL');
    }
    await exited;
  })();
  const server = net.createServer(socket => {
    socket.setEncoding('utf8');
    let request = '';
    socket.on('error', () => {});
    socket.on('data', async chunk => {
      request += chunk;
      if (!request.endsWith('\n')) return;
      socket.pause();
      try {
        const owner = JSON.parse(request);
        const pid = Number(windows ? owner.native_pid : owner.pid);
        if (pid !== child.pid || ownerState(owner) !== 'alive') throw new Error('Supervisor owner identity mismatch');
        await stop();
        socket.end('stopped\n');
      } catch (error) { socket.end(`Cancellation failed: ${error.message}\n`); }
    });
  });
  server.listen(endpoint);
  await once(server, 'listening');
  if (!windows) fs.chmodSync(endpoint, 0o600);
  const bash = bashInterpreter();
  const nativeBash = windows ? path.resolve(path.dirname(bash), '../usr/bin/bash.exe') : bash;
  child = spawn(fs.existsSync(nativeBash) ? nativeBash : bash, [script, ...commandArgs], {
    detached: !windows, stdio: 'inherit',
    env: { ...process.env, CT_GATEWAY_SUPERVISOR_ENDPOINT: endpoint },
  });
  exited = once(child, 'exit');
  const signals = ['SIGINT', 'SIGTERM', 'SIGHUP'];
  const onSignal = () => { stop().catch(error => { console.error(error.message); process.exitCode = 1; }); };
  for (const signal of signals) process.on(signal, onSignal);
  try {
    const [code, signal] = await exited;
    process.exitCode = code ?? 128 + os.constants.signals[signal];
  } finally {
    server.close();
    for (const signal of signals) process.off(signal, onSignal);
  }
}

try {
  if (operation === 'run') await run(value, args);
  else if (operation === 'cancel') await cancel(value);
  else throw new Error('Expected run or cancel');
} catch (error) { console.error(error.message); process.exitCode = 1; }
