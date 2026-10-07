import { spawn } from 'node:child_process';
import { shellCommand } from './runtime.mjs';

export async function run(command, cwd, milliseconds, dependencies = {}) {
  const launch = dependencies.spawn || spawn;
  const platform = dependencies.platform || process.platform;
  const arm = dependencies.arm || setTimeout;
  const disarm = dependencies.disarm || clearTimeout;
  const killGroup = dependencies.killGroup || process.kill;
  const windows = platform === 'win32';
  const text = shellCommand(command, platform);
  const args = windows ? ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(text, 'utf16le').toString('base64')] : ['-c', `exec ${text}`];
  return await new Promise(resolve => {
    const child = launch(windows ? 'powershell.exe' : '/bin/sh', args, { cwd, detached: !windows, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '', timedOut = false, error, termination = Promise.resolve(true), cleanupDeadline, finished = false;
    const finish = result => { if (finished) return; finished = true; if (cleanupDeadline) disarm(cleanupDeadline); resolve(result); };
    for (const stream of [child.stdout, child.stderr]) stream.on('data', chunk => { output = (output + chunk).slice(-32768); });
    const deadline = arm(() => {
      timedOut = true;
      cleanupDeadline = arm(() => {
        child.stdout.destroy?.(); child.stderr.destroy?.(); child.unref?.();
        finish({ command, code: null, timeout: true, contained: false, error: 'process settlement unconfirmed at cleanup deadline', output });
      }, 5000);
      if (windows) termination = new Promise(settled => {
        const killer = launch('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore', timeout: 4000 });
        killer.once('error', cause => { error = cause.message; settled(false); });
        killer.once('close', code => settled(code === 0));
      });
      else { try { killGroup(-child.pid, 'SIGKILL'); } catch (cause) { error = String(cause); termination = Promise.resolve(false); } }
    }, milliseconds);
    child.once('error', cause => { error = cause.message; });
    child.once('close', async (code, signal) => {
      disarm(deadline);
      const contained = await termination;
      finish({ command, code, signal, timeout: timedOut, error, contained, output });
    });
  });
}

