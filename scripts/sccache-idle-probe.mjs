// Caller: scripts/cache-policy.sh on credentialed self-hosted CI.
import {spawnSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {bashInterpreter} from './bash-interpreter.mjs';
import {serverProcessSnapshot} from './sccache-server-process.mjs';
import {serverAddress} from './sccache-server-probe.mjs';

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {encoding: 'utf8', timeout: 5000, windowsHide: true, ...options});
  if (result.error || result.status !== 0) throw new Error(`${command} process evidence unavailable`);
  return result.stdout.trim();
}

function gatewayIdle(script) {
  const raw = run(bashInterpreter(), ['-c', 'source "$1"; ct_gateway_status', 'sccache-idle', script]);
  const status = JSON.parse(raw);
  return status.state === 'idle' && status.active === null && status.queue.length === 0;
}

function activeClients(serverPids) {
  const serverIds = new Set(serverPids);
  if (process.platform === 'win32') {
    const script = "Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^(cargo|rustc|sccache)(\\.exe)?$' } | Select-Object ProcessId,Name | ConvertTo-Json -Compress";
    const raw = run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script]);
    const rows = raw ? [].concat(JSON.parse(raw)) : [];
    return rows.filter(row => !serverIds.has(Number(row.ProcessId)));
  }
  const raw = run('ps', ['-A', '-o', 'pid=,comm=']);
  return raw.split(/\r?\n/).flatMap(line => {
    const match = line.trim().match(/^(\d+)\s+(\S+)$/);
    if (!match || !/^(cargo|rustc|sccache)(\.exe)?$/i.test(match[2]) || serverIds.has(Number(match[1]))) return [];
    return [{pid: Number(match[1]), name: match[2]}];
  });
}

function serverIdentity() {
  if (process.platform === 'win32') return serverProcessSnapshot();
  const address = serverAddress();
  if (address.path) return {available: false, reason: 'Unix socket server identity is not supported by the idle probe'};
  const raw = run('lsof', ['-nP', `-iTCP:${address.port}`, '-sTCP:LISTEN', '-t']);
  const pids = [...new Set(raw.split(/\r?\n/).map(Number).filter(Number.isSafeInteger))];
  if (!pids.length) return {available: false, reason: 'sccache listener process unavailable'};
  const processes = run('ps', ['-p', pids.join(','), '-o', 'pid=,comm=']).split(/\r?\n/).flatMap(line => {
    const match = line.trim().match(/^(\d+)\s+(\S+)$/);
    return match && /^sccache(?:\.exe)?$/i.test(match[2])
      ? [{pid: Number(match[1]), started_at: 'listener'}] : [];
  });
  return processes.length === pids.length ? {available: true, processes} : {available: false, reason: 'listener is not owned by sccache'};
}

export function assertSafeHandover({script, stats, server, clients, idle}) {
  if (!idle) throw new Error('build gateway has an active or queued job');
  if (!server.available || !server.processes?.length) throw new Error('sccache server identity unavailable');
  if (stats.cache_location?.startsWith('Local disk:') !== true
    || !Number.isFinite(stats.stats?.compile_requests)) throw new Error('sccache 0.18 local cache stats unavailable');
  if (clients.length) throw new Error(`active compiler/cache clients: ${clients.map(row => row.name).join(', ')}`);
  return true;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const script = process.argv[2];
    if (!script) throw new Error('build gateway script path required');
    const server = serverIdentity();
    if (!server.available) throw new Error('sccache server process evidence unavailable');
    const stats = JSON.parse(run('sccache', ['--show-stats', '--stats-format', 'json']));
    const clients = activeClients(server.processes.map(row => row.pid));
    assertSafeHandover({script, stats, server, clients, idle: gatewayIdle(script)});
    console.log('[cache-policy] sccache 0.18 stats, gateway state, and process snapshot prove idle');
  } catch (error) {
    console.error(`[cache-policy] refusing local sccache handover: ${error.message}`);
    process.exitCode = 2;
  }
}
