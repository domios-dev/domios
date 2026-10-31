import net from 'node:net';
import { createHash, randomUUID } from 'node:crypto';
import { fork, spawn } from 'node:child_process';
import { realpathSync, rmSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { acquireLaunchFileLock } from './dev-launch-file-lock.mjs';

export function launchLockSocket(root) {
  const physical = realpathSync.native(root);
  const identity = process.platform === 'win32' ? physical.toLowerCase() : physical;
  const key = createHash('sha256').update(identity).digest('hex');
  return process.platform === 'win32' ? `\\\\.\\pipe\\domios-dev-launch-${key}`
    : path.join('/tmp', `domios-launch-${key.slice(0, 40)}.sock`);
}

export async function acquireLaunchOwner(root, { releaseOnReady = false } = {}) {
  const fileLock = await acquireLaunchFileLock(realpathSync.native(root));
  const token = randomUUID();
  const sockets = new Set();
  let finished = false;
  let resolveClosed;
  const closed = new Promise(resolve => { resolveClosed = resolve; });
  let closing = false;
  const server = net.createServer(socket => {
    sockets.add(socket);
    let input = '';
    const deadline = setTimeout(() => socket.destroy(), 5000);
    socket.on('data', bytes => {
      input += bytes.toString();
      if (input.length > 256) socket.destroy();
      else if (input.includes('\n')) {
        clearTimeout(deadline);
        if (releaseOnReady && input === `${token} ready\n`) {
          finished = true;
          for (const peer of sockets) peer.end('released\n');
        }
        else if (input !== `${token}\n`) socket.destroy();
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
    if (finished && sockets.size === 0 && !closing) {
      closing = true;
      server.close(async () => {
        await fileLock.release();
        resolveClosed();
      });
    }
  }
  let socket;
  try {
    socket = launchLockSocket(root);
    if (process.platform !== 'win32') rmSync(socket, { force: true });
    await new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen(socket, resolve);
    });
  } catch (error) { await fileLock.release(); throw error; }
  fileLock.child.once('close', () => {
    if (!closing) {
      for (const socket of sockets) socket.destroy();
      finished = true;
      closeIfFinished();
    }
  });
  return {
    environment: { CODETERM_DEV_LAUNCH_SOCKET: socket, CODETERM_DEV_LAUNCH_TOKEN: token },
    finish: () => { finished = true; closeIfFinished(); return closed; },
  };
}

export async function retainLaunchOwner(environment = process.env) {
  if (!environment.CODETERM_DEV_LAUNCH_TOKEN) return null;
  return new Promise((resolve, reject) => {
    const socket = net.connect(environment.CODETERM_DEV_LAUNCH_SOCKET);
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

export function targetOwnerEnvironment(environment = process.env) {
  return { CODETERM_DEV_LAUNCH_SOCKET: environment.CODETERM_DEV_TARGET_SOCKET,
    CODETERM_DEV_LAUNCH_TOKEN: environment.CODETERM_DEV_TARGET_TOKEN };
}

export async function validateTargetReservation(root, environment = process.env) {
  if (!environment.CODETERM_DEV_TARGET_TOKEN || environment.CODETERM_DEV_TARGET_SOCKET !== launchLockSocket(root)) {
    throw Error('DEV target reservation has no matching live lease');
  }
  const lease = await retainLaunchOwner(targetOwnerEnvironment(environment));
  await new Promise(resolve => { lease.once('close', resolve); lease.end(); });
}

export async function releaseTargetReservation(environment = process.env) {
  if (!environment.CODETERM_DEV_TARGET_TOKEN) return;
  await new Promise((resolve, reject) => {
    const socket = net.connect(environment.CODETERM_DEV_TARGET_SOCKET);
    const deadline = setTimeout(() => socket.destroy(Error('DEV target release deadline')), 5000);
    socket.once('connect', () => socket.write(`${environment.CODETERM_DEV_TARGET_TOKEN} ready\n`));
    socket.on('data', () => {});
    socket.once('end', () => { clearTimeout(deadline); resolve(); });
    socket.once('error', error => { clearTimeout(deadline); reject(error); });
  });
}

async function withTargetReservation() {
  const [root, shell, ...args] = process.argv.slice(3);
  let owner;
  try { owner = await acquireLaunchOwner(root, { releaseOnReady: true }); }
  catch (error) {
    if (error.code !== 'DEV_LAUNCH_BUSY') throw error;
    throw Object.assign(Error(`Checkout target ${root} is reserved by another named DEV build or launch. Wait for its native READY or exit, then rerun this command.`),
      { code: 'DEV_TARGET_BUSY', exitCode: 75 });
  }
  const child = spawn(shell, args, { stdio: 'inherit', windowsHide: true, env: { ...process.env,
    CODETERM_DEV_TARGET_CHILD: '1', CODETERM_DEV_TARGET_SOCKET: owner.environment.CODETERM_DEV_LAUNCH_SOCKET,
    CODETERM_DEV_TARGET_TOKEN: owner.environment.CODETERM_DEV_LAUNCH_TOKEN } });
  const interrupt = () => child.kill('SIGINT');
  const terminate = () => child.kill('SIGTERM');
  process.on('SIGINT', interrupt);
  process.on('SIGTERM', terminate);
  try {
    process.exitCode = await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => resolve(code ?? (signal ? 1 : 0)));
    });
  } finally {
    await owner.finish();
    process.off('SIGINT', interrupt);
    process.off('SIGTERM', terminate);
  }
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
        if (message.error) reject(Object.assign(Error(message.error), { code: message.code, exitCode: message.exitCode }));
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
    : process.argv[2] === '--target-reservation' ? withTargetReservation
      : process.argv[2] === '--validate-target-reservation' ? () => validateTargetReservation(process.argv[3]) : main;
  operation().catch(error => {
    if (process.connected) process.send({ error: error.message, code: error.code, exitCode: error.exitCode }, () => process.disconnect());
    else console.error(`${error.exitCode === 75 ? `${error.code}: ` : ''}${error.message}`);
    process.exitCode = error.exitCode ?? 1;
  });
}
