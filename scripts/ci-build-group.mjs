import { spawn } from 'node:child_process';

// The group leader stays alive until killpg so its PID cannot be reused during cleanup.
const stop = () => process.kill(-process.pid, 'SIGKILL');
process.on('disconnect', stop);
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, stop);
if (!process.connected) stop();

const [program, ...args] = process.argv.slice(2);
const child = spawn(program, args, { stdio: 'inherit' });
const finish = result => {
  if (!process.connected) return stop();
  process.send({ type: 'result', ...result }, stop);
};
child.once('error', error => finish({ error: error.message }));
child.once('exit', (code, signal) => finish({ code, signal }));
