// Caller: build.sh check|check-tests|test-one|release-preflight|install-local|build|test|verify|fmt-check; scripts tests/reap-dev-sidecars.test.mjs; skill dev-run
import { execFileSync } from 'node:child_process';
import { ownedGroups, ownedSidecar } from './dev-sidecar-ownership.mjs';

const dataDir = process.argv[2];
if (!dataDir) throw new Error('Instance data directory required');
const windows = process.platform === 'win32';
function snapshot() {
  if (windows) {
    const json = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      'ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process | Select-Object ProcessId,CreationDate,CommandLine)'], { encoding: 'utf8' });
    return JSON.parse(json).map(p => ({ pid: p.ProcessId, identity: p.CreationDate, command: p.CommandLine ?? '' }));
  }
  return execFileSync('ps', ['-axo', 'pid=,pgid=,stat=,lstart=,command='], { encoding: 'utf8' })
    .split('\n').flatMap(line => {
      const match = line.match(/^\s*(\d+)\s+(\d+)\s+(\S+)\s+(\S+\s+\S+\s+\d+\s+\S+\s+\d+)\s+(.*)$/);
      return match && !match[3].startsWith('Z') ? [{ pid: Number(match[1]), group: Number(match[2]), identity: match[4], command: match[5] }] : [];
    });
}
const processes = snapshot();
const owned = processes.filter(p => ownedSidecar(p.command, dataDir));
if (process.argv[3] === '--count') {
  console.log(owned.length);
  process.exit(0);
}
const groups = ownedGroups(processes, dataDir);
if (!windows && owned.some(p => !groups.some(group => group.pid === p.group))) {
  throw new Error('Refusing a sidecar without an identified owned process-group leader');
}
for (const candidate of groups) {
  const verified = ownedGroups(snapshot(), dataDir).find(p => p.pid === candidate.pid && p.identity === candidate.identity);
  if (!verified) continue;
  if (windows) {
    execFileSync('taskkill.exe', ['/PID', String(verified.pid), '/T', '/F'], { stdio: 'ignore' });
  } else {
    try { process.kill(-verified.group, 'SIGKILL'); }
    catch (error) { if (error.code !== 'ESRCH') throw error; }
  }
  console.log(`Reaped dev Codex sidecar group ${verified.pid}`);
}
