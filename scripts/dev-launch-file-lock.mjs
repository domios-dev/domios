import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

export async function acquireLaunchFileLock(root) {
  const windows = process.platform === 'win32';
  const script = fileURLToPath(new URL(windows ? './windows-dev-launch-file-lock.ps1' : './dev-launch-file-lock.py', import.meta.url));
  const lock = path.join(root, '.dev-launch.lock');
  const child = spawn(windows ? 'powershell.exe' : 'python3', windows
    ? ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, '-Path', lock]
    : [script, lock], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  let detail = '';
  child.stdin.on('error', () => {});
  child.stderr.on('data', bytes => { detail = `${detail}${bytes}`.slice(-4096); });
  const exited = new Promise(resolve => child.once('close', resolve));
  try {
    await new Promise((resolve, reject) => {
      const deadline = setTimeout(() => { child.kill(); reject(Error('DEV launch file lock acquisition deadline')); }, 10000);
      let reply = '';
      child.stdout.on('data', bytes => {
        reply += bytes.toString();
        if (reply === 'locked\r\n' || reply === 'locked\n') { clearTimeout(deadline); resolve(); }
        else if (reply.length > 128) { clearTimeout(deadline); reject(Error('Invalid DEV file lock acknowledgement')); }
      });
      child.once('error', error => { clearTimeout(deadline); reject(error); });
      child.once('close', () => { clearTimeout(deadline); reject(Error(`DEV launch owner unavailable for ${root}: ${detail.trim()}`)); });
    });
  } catch (error) { child.stdin.end(); throw error; }
  return { child, release: async () => { child.stdin.end(); await exited; } };
}
