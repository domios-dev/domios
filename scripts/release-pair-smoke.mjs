#!/usr/bin/env node
import { randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { chmod, lstat, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HEADLESS_TARGETS, headlessAssetNames, readHeadlessArchive, validateHeadlessStage } from './headless-release-package.mjs';
import { verifyHeadlessAssets } from './release-headless-assets.mjs';
import { validateManifest } from './package-server-component-manifest.mjs';

function fail(message) { throw new Error(message); }

export function parseApiToken(config) {
  const section = /^\s*\[([^\]]+)\]\s*$/;
  let inApi = false;
  let token;
  for (const line of config.split(/\r?\n/)) {
    const header = section.exec(line);
    if (header) { inApi = header[1] === 'api'; continue; }
    if (!inApi) continue;
    const match = /^\s*token\s*=\s*("(?:[^"\\]|\\.)*")\s*(?:#.*)?$/.exec(line);
    if (match) {
      if (token !== undefined) fail('isolated API config has duplicate tokens');
      token = JSON.parse(match[1]);
    }
  }
  if (typeof token !== 'string' || !token || /[\r\n]/.test(token)) fail('isolated API token is missing or invalid');
  return token;
}

export function validateReady(line, build, port) {
  const ready = JSON.parse(line);
  if (JSON.stringify(Object.keys(ready).sort()) !== JSON.stringify(['build', 'port', 'type']) ||
      ready.type !== 'READY' || ready.build !== build || ready.port !== port) fail('isolated server READY identity differs from packaged manifest');
  return ready;
}

async function choosePort() {
  const server = createServer();
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = server.address().port;
  await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  if ([0, 7685, 7686].includes(port)) fail('isolated port is reserved');
  return port;
}

function deadlineSignal(ms) { return AbortSignal.timeout(ms); }

function waitForReady(child, build, port, signal) {
  return new Promise((resolve, reject) => {
    let pending = '';
    const cleanup = () => { child.stdout.off('data', onData); child.off('exit', onExit); signal.removeEventListener('abort', onAbort); };
    const finish = (error, value) => { cleanup(); error ? reject(error) : resolve(value); };
    const onExit = () => finish(new Error('isolated server exited before READY'));
    const onAbort = () => finish(new Error('isolated server READY deadline expired'));
    const onData = bytes => {
      pending += bytes.toString('utf8');
      if (pending.length > 8192) return finish(new Error('isolated server READY output is too large'));
      const end = pending.indexOf('\n');
      if (end < 0) return;
      try { finish(null, validateReady(pending.slice(0, end).trim(), build, port)); }
      catch { finish(new Error('isolated server emitted an invalid READY line')); }
    };
    child.stdout.on('data', onData);
    child.once('exit', onExit);
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

function waitForExit(child, signal) {
  if (child.exitCode !== null) return Promise.resolve(child.exitCode);
  return new Promise((resolve, reject) => {
    const cleanup = () => { child.off('exit', onExit); child.off('error', onError); signal.removeEventListener('abort', onAbort); };
    const onExit = code => { cleanup(); resolve(code); };
    const onError = () => { cleanup(); reject(new Error('isolated child failed to launch')); };
    const onAbort = () => { cleanup(); reject(new Error('isolated child exit deadline expired')); };
    child.once('exit', onExit);
    child.once('error', onError);
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

async function runCli(binary, env, signal) {
  const child = spawn(binary, ['identify', '--json'], { env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  const chunks = [];
  child.stdout.on('data', bytes => { if (chunks.reduce((size, part) => size + part.length, 0) < 65536) chunks.push(bytes); });
  const code = await waitForExit(child, signal);
  if (code !== 0) fail('packaged CLI authenticated identify failed');
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}

export async function smokePackagedPair({ target, tag, version, sourceSha, stageDirectory, outputDirectory }) {
  const entry = Object.entries(HEADLESS_TARGETS).find(([, value]) => value.target === target);
  if (!entry) fail('unsupported pair-smoke target');
  const [platform, metadata] = entry;
  const stage = await validateHeadlessStage({ stageDirectory, target, tag, version, sourceSha });
  const assets = await verifyHeadlessAssets({ tag, platform, directory: outputDirectory, sourceSha });
  const archive = readHeadlessArchive(assets.bytes[0]);
  if (archive.size !== stage.files.length) fail('headless archive file count differs from final common stage');
  const scratch = await mkdtemp(path.join(os.tmpdir(), 'domios-pair-smoke-'));
  let child;
  try {
    for (const file of stage.files) {
      const packed = archive.get(file.path);
      if (!packed || !packed.bytes.equals(file.bytes)) fail('headless archive differs from final common stage: ' + file.path);
      const output = path.join(scratch, file.path);
      await mkdir(path.dirname(output), { recursive: true });
      await writeFile(output, packed.bytes, { flag: 'wx' });
      if (process.platform !== 'win32') await chmod(output, packed.mode);
    }
    const manifest = validateManifest(JSON.parse((await readFile(path.join(scratch, 'server-component.json'))).toString('utf8')));
    if (manifest.client_identity.kind !== 'none' || manifest.server_build !== sourceSha) fail('headless component identity differs from source');
    const data = path.join(scratch, 'isolated-data');
    const config = path.join(scratch, 'isolated-config');
    await Promise.all([mkdir(data), mkdir(config)]);
    const port = await choosePort();
    const server = path.join(scratch, 'domios-server', metadata.server);
    const cli = path.join(scratch, 'cli', metadata.cli);
    for (const file of [server, cli]) if (!(await lstat(file)).isFile()) fail('packaged executable is missing');
    child = spawn(server, ['start', '--verify-isolated', '--data-dir', data, '--config-dir', config, '--port', String(port)],
      { cwd: scratch, env: { ...process.env, CODETERM_API_PORT: '', CODETERM_DATA_DIR: data },
        stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    const deadline = deadlineSignal(30000);
    await waitForReady(child, manifest.server_build, port, deadline);
    const healthResponse = await fetch(`http://127.0.0.1:${port}/api/health`, { signal: deadline });
    if (!healthResponse.ok || (await healthResponse.json()).build !== manifest.server_build) fail('packaged server health build differs from manifest');
    const token = parseApiToken(await readFile(path.join(config, 'config.toml'), 'utf8'));
    const nonce = randomUUID();
    const env = { ...process.env, CODETERM_DATA_DIR: data, CODETERM_API_URL: `http://127.0.0.1:${port}`,
      CODETERM_API_TOKEN: token, CODETERM_INSTANCE: `verify-smoke-${nonce}`,
      CODETERM_SOCKET_PATH: `codeterm-dev-verify-smoke-${nonce}.sock` };
    delete env.CODETERM_API_PORT;
    const identity = await runCli(cli, env, deadline);
    if (identity.cli?.build_sha !== sourceSha || identity.connection?.instance !== `verify-smoke-${nonce}`) {
      fail('packaged CLI identity differs from source or isolated instance');
    }
    child.stdin.end();
    if (await waitForExit(child, deadline) !== 0) fail('isolated packaged server did not exit cleanly');
    return { target, sourceSha, serverBuild: manifest.server_build, port, health: 'passed', authenticatedCli: 'passed', shutdown: 'clean' };
  } finally {
    if (child && child.exitCode === null) {
      child.kill();
      await waitForExit(child, deadlineSignal(5000)).catch(() => {});
    }
    await rm(scratch, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [target, tag, sourceSha, stageDirectory, outputDirectory] = process.argv.slice(2);
  smokePackagedPair({ target, tag, version: tag?.slice(1), sourceSha, stageDirectory, outputDirectory })
    .then(result => process.stdout.write(JSON.stringify(result) + '\n'))
    .catch(error => { process.stderr.write('release pair smoke refused: ' + error.message + '\n'); process.exitCode = 1; });
}
