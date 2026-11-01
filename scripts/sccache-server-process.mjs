// Caller: scripts/build-gateway-evidence.mjs
import { spawnSync } from 'node:child_process';
import { serverAddress } from './sccache-server-probe.mjs';

export function parseWindowsProcess(raw) {
  if (!raw.trim()) return null;
  try {
    const value = JSON.parse(raw);
    if (!value || !Number.isSafeInteger(Number(value.pid)) || !value.started_at) return null;
    return { pid: Number(value.pid), started_at: value.started_at };
  } catch { return null; }
}

export function parsePosixProcesses(raw) {
  return raw.split(/\r?\n/).flatMap(line => {
    const match = line.match(/^\s*(\d+)\s+(.{24})\s+(.+)$/);
    if (!match || !/^sccache(?:\.exe)?$/i.test(match[3].split('/').at(-1))) return [];
    return [{ pid: Number(match[1]), started_at: match[2].trim() }];
  });
}

export function serverProcessSnapshot({ platform = process.platform, env = process.env, run = spawnSync } = {}) {
  if (platform === 'win32') {
    const port = serverAddress(env, platform).port;
    const script = `$listener=Get-NetTCPConnection -State Listen -LocalPort ${port} -ErrorAction SilentlyContinue | Select-Object -First 1; if ($listener) { $p=Get-CimInstance Win32_Process -Filter \"ProcessId=$($listener.OwningProcess)\"; if ($p) { [pscustomobject]@{pid=$p.ProcessId;started_at=$p.CreationDate} | ConvertTo-Json -Compress } }`;
    const result = run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8', timeout: 5000, windowsHide: true });
    if (result.status !== 0) return { available: false, reason: 'process query failed' };
    const process = parseWindowsProcess(result.stdout);
    return process ? { available: true, processes: [process] } : { available: false, reason: 'listener process unavailable' };
  }
  const result = run('ps', ['-A', '-o', 'pid=,lstart=,comm='], { encoding: 'utf8', timeout: 5000, windowsHide: true });
  if (result.status !== 0) return { available: false, reason: 'process query failed' };
  const processes = parsePosixProcesses(result.stdout);
  return processes.length ? { available: true, processes } : { available: false, reason: 'sccache process unavailable' };
}
