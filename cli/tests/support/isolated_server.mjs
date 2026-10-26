import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, mkdir, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { randomUUID } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';

const [server, cli] = process.argv.slice(2);
assert.ok(server && cli, 'Expected the built server and CLI');
const scratch = await mkdtemp(path.join(os.tmpdir(), 'domios-cli-server-'));
const data = path.join(scratch, 'data');
const config = path.join(scratch, 'config');
const cleanEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) =>
  !key.startsWith('CODETERM_') && !key.startsWith('DOMIOS_')));
const deadline = () => AbortSignal.timeout(30000);
let child;
let exit;
let serverError = '';
let token = '';
const redact = value => String(value).replaceAll(token || '\0', '<redacted>');
async function runCli(args, env, success = true) {
  const process = spawn(cli, args, { env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], signal: deadline() });
  const output = [];
  const errors = [];
  process.stdout.on('data', bytes => output.push(bytes));
  process.stderr.on('data', bytes => errors.push(bytes));
  const [code, signal] = await once(process, 'close');
  const stdout = Buffer.concat(output).toString('utf8');
  const stderr = Buffer.concat(errors).toString('utf8');
  if (!success) return { code, stdout, stderr };
  assert.equal(code, 0, redact('CLI ' + args.join(' ') + ' exited ' + code + '/' + signal +
    '\nstdout: ' + stdout + '\nstderr: ' + stderr));
  return stdout;
}
try {
  await Promise.all([mkdir(data), mkdir(config)]);
  const listener = createServer();
  listener.listen(0, '127.0.0.1');
  await once(listener, 'listening');
  const port = listener.address().port;
  await new Promise((resolve, reject) => listener.close(error => error ? reject(error) : resolve()));
  assert.ok(![0, 7685, 7686].includes(port), 'Reserved port');
  child = spawn(server, ['start', '--verify-isolated', '--data-dir', data, '--config-dir', config, '--port', String(port)],
    { cwd: scratch, env: cleanEnv, detached: process.platform !== 'win32', windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  exit = once(child, 'exit');
  exit.catch(() => {});
  child.stderr.on('data', bytes => { serverError = (serverError + bytes).slice(-8192); });
  const ready = await new Promise((resolve, reject) => {
    const signal = deadline();
    let pending = '';
    const cleanup = () => {
      child.stdout.off('data', onData);
      child.off('error', onError);
      child.off('exit', onExit);
      signal.removeEventListener('abort', onAbort);
    };
    const finish = (error, value) => { cleanup(); error ? reject(error) : resolve(value); };
    const onError = error => finish(error);
    const onExit = code => finish(new Error('Server exited before READY: ' + code + '\n' + serverError));
    const onAbort = () => finish(new Error('Server READY deadline expired\n' + serverError));
    const onData = bytes => {
      pending += bytes;
      if (pending.length > 8192) return finish(new Error('Server READY exceeds bound'));
      if (!pending.includes('\n')) return;
      try { finish(null, JSON.parse(pending.slice(0, pending.indexOf('\n')))); }
      catch (error) { finish(error); }
    };
    child.once('error', onError);
    child.once('exit', onExit);
    child.stdout.on('data', onData);
    signal.addEventListener('abort', onAbort, { once: true });
  });
  assert.deepEqual(Object.keys(ready).sort(), ['build', 'port', 'type']);
  assert.equal(ready.type, 'READY');
  assert.equal(ready.port, port);
  const url = 'http://127.0.0.1:' + port;
  const health = await fetch(url + '/api/health', { signal: deadline() });
  assert.equal(health.status, 200);
  assert.equal((await health.json()).build_sha, ready.build);
  token = await runCli(['system', 'api-token'], { ...cleanEnv, CODETERM_DATA_DIR: data });
  assert.ok(token.trim() && !/[\r\n]/.test(token), 'Local token must be one nonempty line');
  const nonce = randomUUID();
  const instance = 'verify-smoke-' + nonce;
  const env = { ...cleanEnv, CODETERM_DATA_DIR: data, CODETERM_API_URL: url, CODETERM_API_TOKEN: token,
    CODETERM_INSTANCE: instance, CODETERM_SOCKET_PATH: 'domios-dev-' + instance + '.sock' };
  const missingPort = await runCli(['identify', '--json'], env, false);
  assert.notEqual(missingPort.code, 0, 'A callback URL alone must not override the selected instance');
  assert.match(missingPort.stdout, /Development API port is absent from the selected configuration/);
  assert.equal(missingPort.stderr, '', 'JSON CLI errors are reported on stdout');
  assert.ok(!missingPort.stdout.includes(token), 'Refusal must not expose the credential');
  process.stdout.write('Original smoke identify refusal: ' + redact(missingPort.stdout.trim()) + '; stderr empty\n');
  env.CODETERM_API_PORT = String(port);
  const unauthenticated = await runCli(['identify', '--json'], { ...env, CODETERM_API_TOKEN: 'invalid-fixture-token' }, false);
  assert.notEqual(unauthenticated.code, 0, 'Identify must require the server credential');
  const identity = JSON.parse(await runCli(['identify', '--json'], env));
  assert.equal(identity.cli.build_sha, ready.build);
  assert.equal(identity.connection.instance, instance);
  assert.equal(identity.connection.port, port);
  assert.equal(path.resolve(identity.connection.data_dir), path.resolve(data));
  child.stdin.end();
  const [code] = await Promise.race([exit, new Promise((_, reject) =>
    deadline().addEventListener('abort', () => reject(new Error('Clean server shutdown deadline expired')), { once: true }))]);
  assert.equal(code, 0, 'Server must shut down cleanly');
  // Rebinding proves the release at once; a refused connect costs about two seconds on Windows.
  const released = createServer();
  released.listen(port, '127.0.0.1');
  await once(released, 'listening');
  await new Promise((resolve, reject) => released.close(error => error ? reject(error) : resolve()));
  process.stdout.write('Server, health, api-token, authenticated identify and clean shutdown passed\n');
} catch (error) {
  const log = await readFile(path.join(data, 'logs', 'domios.log'), 'utf8').catch(() => '');
  process.stderr.write(redact(error.stack + '\nServer stderr: ' + serverError + '\n' + log.split(/\r?\n/).slice(-20).join('\n')) + '\n');
  process.exitCode = 1;
} finally {
  if (child?.pid && child.exitCode === null && child.signalCode === null) {
    child.stdin.end();
    try {
      await Promise.race([exit, new Promise((_, reject) =>
        AbortSignal.timeout(10000).addEventListener('abort', () => reject(new Error('Server cleanup deadline')), { once: true }))]);
    } catch {
      if (process.platform === 'win32') execFileSync('taskkill', ['/F', '/T', '/PID', String(child.pid)], { stdio: 'ignore', windowsHide: true });
      else process.kill(-child.pid, 'SIGKILL');
      await exit;
    }
  }
  await rm(scratch, { recursive: true, force: true });
}
