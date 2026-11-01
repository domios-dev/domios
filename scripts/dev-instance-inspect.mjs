import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export function inspectInstanceProcesses(operation, input) {
  const windows = process.platform === 'win32';
  const script = fileURLToPath(new URL(windows ? './windows-dev-rebuild-stop.ps1' : './unix-dev-instance-processes.py', import.meta.url));
  const result = spawnSync(windows ? 'powershell.exe' : 'python3', windows
    ? ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, '-Operation', operation]
    : [script, operation], { input: JSON.stringify(input), encoding: 'utf8', windowsHide: true, timeout: 55_000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw Error(result.stderr.trim() || 'DEV instance cleanup refused');
  return JSON.parse(result.stdout);
}

