#!/usr/bin/env node
import { randomUUID } from 'node:crypto';
import { execFile, spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { appendFile, chmod, lstat, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { promisify } from 'node:util';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HEADLESS_TARGETS, headlessAssetNames, readHeadlessArchive, validateHeadlessStage } from './headless-release-package.mjs';
import { verifyHeadlessAssets } from './release-headless-assets.mjs';
import { validateManifest } from './package-server-component-manifest.mjs';

function fail(message) { throw new Error(message); }

export function executionPlan(target, hostPlatform = process.platform, hostArch = process.arch) {
  const targetArch = target.startsWith('aarch64-') ? 'arm64' : 'x64';
  const targetPlatform = target.endsWith('-apple-darwin') ? 'darwin'
    : target.endsWith('-pc-windows-msvc') ? 'win32' : 'linux';
  const identity = { host_platform: hostPlatform, host_arch: hostArch, target_arch: targetArch };
  if (hostPlatform === targetPlatform && hostArch === targetArch) return { ...identity, mode: 'native' };
  if (hostPlatform === 'darwin' && targetPlatform === 'darwin' && hostArch === 'arm64' && targetArch === 'x64') {
    return { ...identity, mode: 'rosetta' };
  }
  return { ...identity, mode: 'unverified_cross_arch', reason: 'runner_cannot_execute_target_natively' };
}

async function probeRosetta() {
  try {
    await promisify(execFile)('/usr/bin/arch', ['-x86_64', '/usr/bin/true'], { timeout: 10000 });
    return true;
  } catch {
    return false;
  }
}

export async function resolveExecution(plan, probe = probeRosetta) {
  if (plan.mode !== 'rosetta' || await probe()) return plan;
  return { ...plan, mode: 'unverified_cross_arch', reason: 'rosetta_unavailable_or_failed_to_start' };
}

export function targetCommand(binary, args, execution) {
  if (execution.mode === 'unverified_cross_arch') fail('unverified target must not spawn');
  return execution.mode === 'rosetta'
    ? { binary: '/usr/bin/arch', args: ['-x86_64', binary, ...args] } : { binary, args };
}

export async function recordSmokeReceipt(receipt, outputDirectory, summary = process.env.GITHUB_STEP_SUMMARY) {
  await writeFile(path.join(outputDirectory, `release-pair-smoke-${receipt.target}.json`), JSON.stringify(receipt, null, 2) + '\n');
  if (summary) {
    const verdict = receipt.status === 'unverified_cross_arch' ? 'UNVERIFIED' : 'PASS';
    await appendFile(summary, `\nPair smoke ${receipt.target}: **${verdict}**. Byte checks: ${receipt.byte_checks}. Runtime: ${receipt.status}.` +
      ` Host: ${receipt.execution.host_platform}/${receipt.execution.host_arch}; target: ${receipt.execution.target_arch}.` +
      (receipt.execution.reason ? ` Reason: ${receipt.execution.reason}.` : '') + ` Signing: ${receipt.signing}.\n`);
  }
}

export function validateApiToken(token) {
  if (typeof token !== 'string' || !token.trim() || /[\r\n]/.test(token)) fail('isolated API credential is missing or invalid');
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

export function earlyExitMessage(code, signal) {
  return `isolated server exited before READY (code=${code ?? 'none'}, signal=${signal ?? 'none'})`;
}

export function refusalCapture() {
  const lines = [];
  let pending = '';
  const finishLine = () => {
    if (lines.length < 3 && pending.startsWith('domios-server refused:')) {
      lines.push(pending.replace(/[\u0000-\u001f\u007f]/g, ''));
    }
    pending = '';
  };
  return {
    write(bytes) {
      for (const part of bytes.toString('utf8').split(/(\n)/)) {
        if (part === '\n') finishLine();
        else if (pending.length < 300) pending += part.slice(0, 300 - pending.length);
      }
    },
    finish: finishLine,
    lines: () => [...lines],
  };
}

function waitForReady(child, build, port, signal, refusals) {
  return new Promise((resolve, reject) => {
    let pending = '';
    const cleanup = () => { child.stdout.off('data', onData); child.off('close', onExit); signal.removeEventListener('abort', onAbort); };
    const finish = (error, value) => { cleanup(); error ? reject(error) : resolve(value); };
    const onExit = (code, exitSignal) => {
      refusals.finish();
      finish(new Error([earlyExitMessage(code, exitSignal), ...refusals.lines()].join('\n')));
    };
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
    child.once('close', onExit);
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

async function runCli(binary, env, signal, execution, args = ['identify', '--json']) {
  const command = targetCommand(binary, args, execution);
  const child = spawn(command.binary, command.args, { env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  const chunks = [];
  child.stdout.on('data', bytes => { if (chunks.reduce((size, part) => size + part.length, 0) < 65536) chunks.push(bytes); });
  const code = await waitForExit(child, signal);
  if (code !== 0) fail('packaged CLI authenticated identify failed');
  return Buffer.concat(chunks).toString('utf8');
}

// The supervised server discards its own stderr and the scratch directory is deleted, so its log is the only cause left.
export async function reportServerLogTail(data, { lines = 40, write = text => process.stderr.write(text) } = {}) {
  const log = await readFile(path.join(data, 'logs', 'domios.log'), 'utf8').catch(() => null);
  if (log === null) {
    write('isolated server wrote no log before the failure\n');
    return;
  }
  const tail = log.split(/\r?\n/).filter(Boolean).slice(-lines);
  write(['isolated server log tail:', ...tail].join('\n') + '\n');
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
  const data = path.join(scratch, 'isolated-data');
  let child;
  let outcome;
  let failure = null;
  let residue = null;
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
    const execution = await resolveExecution(executionPlan(target));
    const receipt = { target, sourceSha, serverBuild: manifest.server_build, byte_checks: 'passed', execution,
      signing: platform.startsWith('windows-') ? 'updater-signature only; no Authenticode' : 'platform release signing' };
    if (execution.mode === 'unverified_cross_arch') return { ...receipt, status: 'unverified_cross_arch' };
    const config = path.join(scratch, 'isolated-config');
    await Promise.all([mkdir(data), mkdir(config)]);
    const port = await choosePort();
    const server = path.join(scratch, 'domios-server', metadata.server);
    const cli = path.join(scratch, 'cli', metadata.cli);
    for (const file of [server, cli]) if (!(await lstat(file)).isFile()) fail('packaged executable is missing');
    const command = targetCommand(server, ['start', '--verify-isolated', '--data-dir', data, '--config-dir', config, '--port', String(port)], execution);
    child = spawn(command.binary, command.args,
      { cwd: scratch, env: { ...process.env, CODETERM_API_PORT: '', CODETERM_DATA_DIR: data },
        stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    const refusals = refusalCapture();
    child.stderr.on('data', bytes => refusals.write(bytes));
    const deadline = deadlineSignal(30000);
    await waitForReady(child, manifest.server_build, port, deadline, refusals);
    const healthResponse = await fetch(`http://127.0.0.1:${port}/api/health`, { signal: deadline });
    if (!healthResponse.ok || (await healthResponse.json()).build_sha !== manifest.server_build) fail('packaged server health build differs from manifest');
    const tokenEnv = { ...process.env, CODETERM_DATA_DIR: data };
    for (const key of ['CODETERM_API_TOKEN', 'CODETERM_API_URL', 'CODETERM_API_PORT', 'CODETERM_SOCKET_PATH', 'CODETERM_INSTANCE']) delete tokenEnv[key];
    const token = validateApiToken(await runCli(cli, tokenEnv, deadline, execution, ['system', 'api-token']));
    const nonce = randomUUID();
    const env = { ...process.env, CODETERM_DATA_DIR: data, CODETERM_API_URL: `http://127.0.0.1:${port}`,
      CODETERM_API_TOKEN: token, CODETERM_INSTANCE: `verify-smoke-${nonce}`,
      CODETERM_SOCKET_PATH: `domios-dev-verify-smoke-${nonce}.sock` };
    delete env.CODETERM_API_PORT;
    const identity = JSON.parse(await runCli(cli, env, deadline, execution));
    if (identity.cli?.build_sha !== sourceSha || identity.connection?.instance !== `verify-smoke-${nonce}`) {
      fail('packaged CLI identity differs from source or isolated instance');
    }
    child.stdin.end();
    if (await waitForExit(child, deadline) !== 0) fail('isolated packaged server did not exit cleanly');
    outcome = { ...receipt, status: 'passed', port, health: 'passed', authenticatedCli: 'passed', shutdown: 'clean' };
  } catch (error) {
    await reportServerLogTail(data);
    failure = error;
  } finally {
    if (child && child.exitCode === null) {
      child.kill();
      await waitForExit(child, deadlineSignal(5000)).catch(() => {});
    }
    residue = await rm(scratch, { recursive: true, force: true }).then(() => null, error => error);
  }
  if (failure) throw failure;
  if (residue) {
    await reportServerLogTail(data);
    throw new Error(scratchResidueMessage(residue));
  }
  return outcome;
}

// A clean server exit followed by a busy scratch directory means a child it started is still writing there.
export function scratchResidueMessage(error) {
  const where = error?.path ? ' at ' + error.path : '';
  return `isolated server left a writer behind in its scratch directory${where}: ${error?.code ?? error?.message ?? error}`;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [target, tag, sourceSha, stageDirectory, outputDirectory] = process.argv.slice(2);
  smokePackagedPair({ target, tag, version: tag?.slice(1), sourceSha, stageDirectory, outputDirectory })
    .then(async result => { await recordSmokeReceipt(result, outputDirectory); process.stdout.write(JSON.stringify(result) + '\n'); })
    .catch(error => { process.stderr.write('release pair smoke refused: ' + error.message + '\n'); process.exitCode = 1; });
}
