// Caller: dev-prep.mjs; tests/dev-app-process.test.mjs
import { spawn, spawnSync } from 'node:child_process';
import { closeSync, fstatSync, mkdirSync, openSync, readSync, watch, writeSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export function runDevApp({ app, args = [], log, cwd, env, onData = () => {} }) {
  return new Promise((resolveExit, rejectExit) => {
    mkdirSync(dirname(log), { recursive: true });
    // Rust stdio requires synchronous handles; libuv child pipes are overlapped on Windows.
    const windows = process.platform === 'win32';
    const logHandle = openSync(log, 'w');
    let readHandle;
    try { readHandle = openSync(log, 'r'); } catch (error) { closeSync(logHandle); throw error; }
    let offset = 0;
    let watcher;
    let child;
    let finished = false;
    const buffer = Buffer.alloc(64 * 1024);
    const drain = () => {
      const size = fstatSync(readHandle).size;
      while (offset < size) {
        const count = readSync(readHandle, buffer, 0, Math.min(buffer.length, size - offset), offset);
        if (!count) break;
        offset += count;
        onData(Buffer.from(buffer.subarray(0, count)));
      }
    };
    const stopChild = signal => {
      if (!child?.pid || child.exitCode !== null) return;
      if (windows) spawnSync('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true, timeout: 10_000 });
      else child.kill(signal);
    };
    const forwardInterrupt = () => stopChild('SIGINT');
    const forwardTerminate = () => stopChild('SIGTERM');
    const finish = (error, code, signal) => {
      if (finished) return;
      finished = true;
      try { if (!windows) drain(); } catch (drainError) { error ??= drainError; }
      watcher?.close();
      closeSync(readHandle);
      closeSync(logHandle);
      process.off('SIGINT', forwardInterrupt);
      process.off('SIGTERM', forwardTerminate);
      if (error) rejectExit(error);
      else resolveExit({ code, signal });
    };
    const failed = error => {
      stopChild('SIGTERM');
      finish(error);
    };
    try {
      if (windows) {
        const wrapper = fileURLToPath(new URL('./windows-dev-app.ps1', import.meta.url));
        child = spawn('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', wrapper, '-App', app], {
          cwd, env: { ...env, CODETERM_DEV_APP_ARGS: JSON.stringify(args) },
          stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
        });
        const consume = chunk => {
          if (finished) return;
          try { writeSync(logHandle, chunk); onData(chunk); } catch (error) { failed(error); }
        };
        child.stdout.on('data', consume);
        child.stderr.on('data', consume);
      } else {
        watcher = watch(log, () => {
          if (finished) return;
          try { drain(); } catch (error) { failed(error); }
        });
        watcher.on('error', failed);
        child = spawn(app, args, { cwd, env, stdio: ['ignore', logHandle, logHandle] });
      }
      child.once('error', failed);
      child.once('close', (code, signal) => finish(undefined, code, signal));
      process.on('SIGINT', forwardInterrupt);
      process.on('SIGTERM', forwardTerminate);
    } catch (error) { failed(error); }
  });
}
