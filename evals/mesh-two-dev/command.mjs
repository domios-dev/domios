import { spawn } from 'node:child_process';
export function runDc(libPath, args, deadlineMs = 20000) {
  return new Promise((resolve, reject) => {
    const child = spawn('bash', ['-c', 'set -euo pipefail; source "$1"; dc "${@:2}"', 'mesh-two-dev', libPath, ...args], {
      env: { ...process.env, CODETERM_INSTANCE: 'aiderqa', CODETERM_API_PORT: '7697' },
      signal: AbortSignal.timeout(deadlineMs),
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '', stderr = '';
    child.stdout.on('data', data => { stdout += data; });
    child.stderr.on('data', data => { stderr += data; });
    child.on('error', error => reject(Object.assign(error, { stdout, stderr })));
    child.on('close', (code, signal) => resolve({ code, signal, stdout, stderr, args }));
  });
}
