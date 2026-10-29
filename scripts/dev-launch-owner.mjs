import net from 'node:net';
import { createHash, randomUUID } from 'node:crypto';
import { fork, spawn } from 'node:child_process';
import { realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function launchLockPort(root) {
  const physical = realpathSync.native(root);
  const identity = process.platform === 'win32' ? physical.toLowerCase() : physical;
  return 30000 + createHash('sha256').update(identity).digest().readUInt32BE() % 30000;
}

export async function acquireLaunchOwner(root) {
  const token = randomUUID();
  const sockets = new Set();
  let finished = false;
  let resolveClosed;
  const closed = new Promise(resolve => { resolveClosed = resolve; });
  const server = net.createServer(socket => {
    sockets.add(socket);
    let input = '';
    const deadline = setTimeout(() => socket.destroy(), 5000);
    socket.on('data', bytes => {
      input += bytes.toString();
      if (input.length > 256) socket.destroy();
      else if (input.includes('\n')) {
        clearTimeout(deadline);
        if (input !== `${token}\n`) socket.destroy();
        else socket.write('owned\n');
      }
    });
    socket.on('error', () => {});
    socket.on('close', () => {
      clearTimeout(deadline);
      sockets.delete(socket);
      closeIfFinished();
    });
  });
  function closeIfFinished() {
    if (finished && sockets.size === 0) server.close(resolveClosed);
  }
  const port = launchLockPort(root);
  await new Promise((resolve, reject) => {
    server.once('error', error => reject(Error(`DEV launch owner unavailable for ${root} (lock port ${port}): ${error.code}. Stop the current launcher with scoped build.sh kill or let it exit.`)));
    server.listen({ host: '127.0.0.1', port, exclusive: true }, resolve);
  });
  return {
    environment: { CODETERM_DEV_LAUNCH_PORT: String(port), CODETERM_DEV_LAUNCH_TOKEN: token },
    finish: () => { finished = true; closeIfFinished(); return closed; },
  };
}

export async function retainLaunchOwner(environment = process.env) {
  if (!environment.CODETERM_DEV_LAUNCH_TOKEN) return null;
  return new Promise((resolve, reject) => {
    const socket = net.connect({ host: '127.0.0.1', port: Number(environment.CODETERM_DEV_LAUNCH_PORT) });
    const deadline = setTimeout(() => socket.destroy(Error('DEV launch ownership handshake deadline')), 5000);
    let input = '';
    socket.once('connect', () => socket.write(`${environment.CODETERM_DEV_LAUNCH_TOKEN}\n`));
    socket.on('data', bytes => {
      input += bytes.toString();
      if (input === 'owned\n') { clearTimeout(deadline); resolve(socket); }
      else if (input.length > 256) socket.destroy(Error('Invalid DEV launch owner acknowledgement'));
    });
    socket.on('error', error => { clearTimeout(deadline); reject(error); });
    socket.on('close', () => { clearTimeout(deadline); reject(Error('DEV launch owner disconnected')); });
  });
}

async function main() {
  const [root, shell, ...args] = process.argv.slice(2);
  let environment = process.env;
  let broker;
  if (!environment.CODETERM_DEV_LAUNCH_TOKEN) {
    broker = fork(fileURLToPath(import.meta.url), ['--serve-lock', root], {
      detached: true, stdio: ['ignore', 'ignore', 'ignore', 'ipc'], windowsHide: true,
    });
    const handoff = await new Promise((resolve, reject) => {
      const deadline = setTimeout(() => { broker.kill(); reject(Error('DEV launch owner handoff deadline')); }, 5000);
      broker.once('message', message => {
        clearTimeout(deadline);
        if (message.error) reject(Error(message.error));
        else resolve(message);
      });
      broker.once('error', error => { clearTimeout(deadline); reject(error); });
      broker.once('exit', () => { clearTimeout(deadline); reject(Error('DEV launch owner exited before handoff')); });
    });
    environment = { ...environment, ...handoff };
  }
  let lease;
  try { lease = await retainLaunchOwner(environment); }
  catch (error) {
    if (broker?.connected) broker.disconnect();
    throw error;
  }
  if (broker) { broker.send('leased'); broker.disconnect(); broker.unref(); }
  const child = spawn(shell, args, {
    stdio: 'inherit', windowsHide: true,
    env: { ...environment, CODETERM_DEV_LAUNCH_CHILD: '1' },
  });
  const interrupt = () => { if (child.exitCode === null) child.kill('SIGINT'); };
  const terminate = () => { if (child.exitCode === null) child.kill('SIGTERM'); };
  process.on('SIGINT', interrupt);
  process.on('SIGTERM', terminate);
  try {
    process.exitCode = await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => resolve(code ?? (signal ? 1 : 0)));
    });
  } finally {
    lease?.end();
    process.off('SIGINT', interrupt);
    process.off('SIGTERM', terminate);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const operation = process.argv[2] === '--serve-lock'
    ? async () => {
      const owner = await acquireLaunchOwner(process.argv[3]);
      process.send(owner.environment);
      await new Promise(resolve => {
        process.once('message', resolve);
        process.once('disconnect', resolve);
      });
      await owner.finish();
    }
    : main;
  operation().catch(error => {
    if (process.connected) process.send({ error: error.message }, () => process.disconnect());
    else console.error(error.message);
    process.exitCode = 1;
  });
}
