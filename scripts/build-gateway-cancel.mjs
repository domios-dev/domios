import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { ownerState, readOwner } from './build-gateway-owner.mjs';

try {
  const owner = readOwner(process.argv[2]);
  if (process.platform !== 'win32' || ownerState(owner) !== 'alive') {
    throw new Error('Native gateway owner identity is unavailable or stale');
  }
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
    '-File', fileURLToPath(new URL('./build-gateway-cancel.ps1', import.meta.url)),
    '-OwnerPid', owner.native_pid, '-OwnerStartToken', owner.native_start_token,
    '-OwnerMsysPid', owner.pid, '-MsysPs', process.argv[3]],
  { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(result.stderr || result.error?.message || 'Native tree cancellation failed');
} catch (error) { console.error(error.message); process.exitCode = 1; }
