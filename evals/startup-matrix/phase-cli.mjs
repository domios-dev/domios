import { spawn, execFile } from 'node:child_process';
import { join, isAbsolute } from 'node:path';
import { accessSync, constants } from 'node:fs';

export function resolveDevCli(env = process.env) {
  const directory = process.platform === 'win32'
    ? env.CODETERM_DEV_DATA_DIR?.replace(/^\/([a-z])\//i, (_, drive) => `${drive}:/`)
    : env.CODETERM_DEV_DATA_DIR;
  if (env.CODETERM_API_PORT !== '7686' || env.CODETERM_API_URL !== 'http://127.0.0.1:7686' ||
      !directory || !isAbsolute(directory) || !env.CODETERM_API_TOKEN ||
      !['codeterm-dev', 'codeterm-dev.sock'].includes(env.CODETERM_SOCKET_PATH) || env.CODETERM_INSTANCE) {
    throw Error('CLI timing requires the default dev_run connection');
  }
  const executable = join(directory, 'bin', process.platform === 'win32' ? 'codeterm.exe' : 'codeterm');
  accessSync(executable, constants.X_OK);
  const childEnv = { ...env, CODETERM_DEV_DATA_DIR: directory, CODETERM_DATA_DIR: directory, CODETERM_TAB_ID: '', CODETERM_PARENT_TAB_ID: '' };
  for (const key of ['CODETERM_STARTUP_ATTEMPT_ID', 'CODETERM_SESSION_BINDING_NONCE', 'CODETERM_SESSION_ID',
    'CODETERM_TEAM_ID', 'CODETERM_PLAN_ID', 'CODETERM_TASK_ID', 'CODETERM_ATTEMPT_ID']) delete childEnv[key];
  return { executable, env: childEnv };
}

export function cliToReceivedMs(requestBeginAt, receivedAtUs) {
  return receivedAtUs == null ? null : receivedAtUs / 1000 - requestBeginAt;
}

export function devCli(root, connection, args, signal, beforeSpawn = () => {}) {
  return new Promise((resolve, reject) => {
    if (signal.aborted) return reject(signal.reason);
    beforeSpawn(Date.now());
    const child = spawn(connection.executable, [...args, '--json'], {
      cwd: root, env: connection.env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
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
        resolve(value.result);
      } catch (error) { reject(error); }
    });
  });
}
