// Caller: dev-prep.mjs; tests/dev-app-process.test.mjs
import { spawn, spawnSync } from 'node:child_process';
import { closeSync, fstatSync, mkdirSync, openSync, readSync, watch, writeSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export function windowsDevArguments({ wrapper, app, args, interactiveDesktop, env }) {
  if (interactiveDesktop && (args.length !== 0 || env.CODETERM_DEV !== '1')) {
    throw new Error('Interactive Windows DEV requires explicit development intent and no arbitrary client arguments');
  }
  return ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', wrapper, '-App', app,
    ...(interactiveDesktop ? ['-InteractiveDesktop'] : [])];
}

export function runDevApp({ app, args = [], log, cwd, env, interactiveDesktop = false, onData = () => {} }) {
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
        child = spawn('powershell.exe', windowsDevArguments({ wrapper, app, args, interactiveDesktop, env }), {
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

export function nativeLocatorRecord(line,record,app,attachment) {
  if (!line.startsWith('DOMIOS_DEV_NATIVE_LOCATOR ')) return null;
  const event=JSON.parse(line.slice('DOMIOS_DEV_NATIVE_LOCATOR '.length));
  const snapshot=event.snapshot, process=snapshot?.client_process, status=snapshot?.status;
  const image = value => typeof value === 'string' ? value.replaceAll('\\','/').replace(/^\/\/\?\//,'').toLowerCase() : null;
  if (record.schema !== 3 || !attachment || attachment.client_pid !== process?.pid ||
      attachment.epoch !== snapshot?.connection?.server_epoch ||
      typeof process?.created !== 'string' || !/^[0-9]+(:[0-9]+)?$/.test(process.created) ||
      image(process.executable) !== image(app) ||
      !Number.isSafeInteger(event.navigation_generation) || event.navigation_generation < 1)
    throw Error('Native DEV locator identity mismatch');
  if (status?.status !== 'active') return {...record,nativeClient:null};
  const origin=new URL(event.origin);
  if (origin.protocol !== 'http:' || origin.hostname !== '127.0.0.1' || ![Number(record.instance.apiPort),Number(record.instance.vitePort)].includes(Number(origin.port)) || origin.origin !== event.origin) throw Error('Native DEV frontend origin mismatch');
  const {port,view_generation}=status.payload;
  if (port !== Number(record.instance.cdpPort) || port < 1024 || port > 65535 || port === 7685 || port === Number(record.instance.apiPort) ||
      !Number.isSafeInteger(view_generation) || view_generation < 1)
    throw Error('Native DEV locator endpoint mismatch');
  return {...record,nativeClient:{platform:'windows',snapshot,navigationGeneration:event.navigation_generation,origin:event.origin}};
}

export function prebuiltReadinessPhase(phase,line) {
  if (!Number.isInteger(phase) || phase < 0 || phase > 2) throw Error('Invalid DEV readiness phase');
  if (!line.startsWith('DOMIOS_CLIENT_DIAGNOSTIC ')) return phase;
  let event;
  try { event=JSON.parse(line.slice('DOMIOS_CLIENT_DIAGNOSTIC '.length)); } catch { return phase; }
  if (event.status !== 'started' || event.exit_code !== null || event.io_kind !== null || event.os_error !== null) return phase;
  if (phase === 0 && event.stage === 'attach') return 1;
  if (phase === 1 && event.stage === 'reply_validation') return 2;
  return phase;
}
