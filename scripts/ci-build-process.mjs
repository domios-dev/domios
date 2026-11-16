import { spawn } from 'node:child_process';
import { constants } from 'node:os';
import { fileURLToPath } from 'node:url';
import { childExit } from './child-exit.mjs';

export function startBuild(program, args, options = {}) {
  const unix = process.platform !== 'win32';
  const child = unix
    ? spawn(process.execPath, [fileURLToPath(new URL('./ci-build-group.mjs', import.meta.url)), program, ...args],
      { detached: true, stdio: ['inherit', 'pipe', 'pipe', 'ipc'] })
    : spawn(program, args, { stdio: ['inherit', 'pipe', 'pipe'] });
  let outcome;
  let cancellation;
  const stopGroup = () => {
    if (!child.pid) return;
    try { process.kill(-child.pid, 'SIGKILL'); }
    catch (error) { if (error.code !== 'ESRCH') throw error; }
  };
  const handlers = new Map();
  if (unix) {
    for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
      const handler = () => { cancellation ??= signal; stopGroup(); };
      handlers.set(signal, handler);
      process.on(signal, handler);
    }
    child.once('spawn', () => { if (cancellation) stopGroup(); });
    child.once('exit', stopGroup);
    child.on('message', message => { if (message?.type === 'result') outcome = message; });
  }
  const result = childExit(child, options).then(observed => {
    if (cancellation) return { ...observed, code: 128 + constants.signals[cancellation] };
    if (outcome?.error) throw new Error(outcome.error);
    if (outcome) return { ...observed, code: outcome.code ?? 128 + constants.signals[outcome.signal] };
    return observed;
  }).finally(() => {
    for (const [signal, handler] of handlers) process.off(signal, handler);
  });
  return { child, result };
}
