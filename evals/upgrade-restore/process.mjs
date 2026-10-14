import {spawn, execFile} from 'node:child_process';

function stop(child) {
  if (!child.pid) return;
  if (process.platform === 'win32') {
    execFile('taskkill', ['/PID', String(child.pid), '/T', '/F'], {timeout: 5000}, () => {});
  } else {
    try { process.kill(-child.pid, 'SIGKILL'); } catch {}
  }
}

export function run(command, args, options) {
  return new Promise((resolve, reject) => {
    const signal = options.signal ?? AbortSignal.timeout(options.timeout);
    const child = (options.spawn ?? spawn)(command, args, {
      detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    let bytes = 0;
    let settled = false;
    function finish(error) {
      if (settled) return;
      settled = true;
      signal.removeEventListener('abort', abort);
      if (error) {
        error.stdout = stdout;
        error.stderr = stderr;
        reject(error);
      } else resolve({stdout, stderr});
    }
    function abort() {
      const error = new Error('Bounded process deadline elapsed');
      error.killed = true;
      finish(error);
      (options.stop ?? stop)(child);
    }
    function output(stream, chunk) {
      bytes += Buffer.byteLength(chunk);
      if (bytes > options.maxBuffer) {
        finish(new Error('Process output exceeds capture bound'));
        (options.stop ?? stop)(child);
        return;
      }
      if (stream === 'stdout') stdout += chunk;
      else stderr += chunk;
    }
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', chunk => output('stdout', chunk));
    child.stderr.on('data', chunk => output('stderr', chunk));
    child.once('error', finish);
    child.once('close', code => finish(code === 0 ? null : new Error(`Process exited ${code}`)));
    signal.addEventListener('abort', abort, {once: true});
    if (signal.aborted) abort();
  });
}
