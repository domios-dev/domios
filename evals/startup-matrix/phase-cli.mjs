import { spawn, execFile } from 'node:child_process';
import { join } from 'node:path';

export function devCli(root, args, signal) {
  return new Promise((resolve, reject) => {
    if (signal.aborted) return reject(signal.reason);
    const child = spawn('bash', [join(root, 'evals/core-journeys/run.sh'), '--dc', ...args, '--json'], {
      cwd: root, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
      detached: process.platform !== 'win32',
    });
    let stdout = '', stderr = '', stopped = false;
    const stop = () => {
      if (stopped) return;
      stopped = true;
      child.stdout.destroy();
      child.stderr.destroy();
      if (child.pid) {
        if (process.platform === 'win32') {
          execFile('taskkill', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, timeout: 10000 }, () => {});
        } else {
          try { process.kill(-child.pid, 'SIGKILL'); } catch (error) {
            if (error.code !== 'ESRCH') reject(error);
          }
        }
      }
      reject(signal.reason ?? Error('CLI output exceeded bound'));
    };
    signal.addEventListener('abort', stop, { once: true });
    if (signal.aborted) stop();
    child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 1024 * 1024) stop(); });
    child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 65536) stop(); });
    child.on('error', reject);
    child.on('close', code => {
      signal.removeEventListener('abort', stop);
      if (stopped) return;
      if (code !== 0) return reject(Error(`DEV CLI exit ${code}: ${stderr.slice(0, 1000)}`));
      try {
        const value = JSON.parse(stdout);
        resolve(value?.data ?? value);
      } catch (error) { reject(error); }
    });
  });
}
