import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { readFileSync, realpathSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

const windows = process.platform === 'win32';
const normalize = value => {
  const normalized = value.replaceAll('\\', '/').replace(/^\/\/\?\//, '');
  return windows ? normalized.toLowerCase() : normalized;
};

export function validateReady(ready, { identity, executable, resources, build, port }) {
  if (ready?.protocol !== 2 || ready.api_version !== 3 || ready.build !== build ||
      !ready.epoch || !ready.instance_id || !Number.isInteger(ready.process?.pid) || ready.process.pid <= 0 ||
      !ready.process.created || normalize(ready.process.executable) !== normalize(executable) ||
      normalize(ready.resources) !== normalize(resources) || ready.api_address !== `127.0.0.1:${port}` ||
      !['data', 'config'].every(key => normalize(ready.identity?.[key] ?? '') === normalize(identity[key]))) {
    throw Error('DEV server readiness differs from the selected instance/artifact');
  }
}

function helper(executable, operation, request, onFrame = () => {}) {
  let child;
  const done = new Promise((resolve, reject) => {
    const wrapper = fileURLToPath(new URL('./dev-server-bootstrap.ps1', import.meta.url));
    child = spawn(windows ? 'powershell.exe' : executable,
      windows ? ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', wrapper, '-Server', executable, '-Operation', operation] : [operation],
      { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    let output = '', bytes = 0;
    const frames = [];
    const deadline = setTimeout(() => {
      child.kill();
      reject(Error(`DEV ${operation} deadline elapsed; no retry was attempted`));
    }, 35_000);
    child.stdout.on('data', chunk => {
      bytes += chunk.length;
      if (bytes > 131072) { child.kill(); return; }
      output += chunk.toString('utf8');
      while (output.includes('\n')) {
        const index = output.indexOf('\n');
        const line = output.slice(0, index); output = output.slice(index + 1);
        try { const frame = JSON.parse(line); frames.push(frame); onFrame(frame); }
        catch { child.kill(); }
      }
    });
    child.stderr.resume();
    child.stdin.on('error', () => {});
    child.once('error', error => { clearTimeout(deadline); reject(error); });
    child.once('close', code => {
      clearTimeout(deadline);
      if (code !== 0 || output.trim() || bytes > 131072) reject(Error(`DEV ${operation} refused (exit ${code})`));
      else resolve(frames);
    });
    child.stdin.end(`${JSON.stringify(request)}\n`);
  });
  return { done, cancel: () => child.kill() };
}

function attachRequest(ready, identity, operation = 'inspect') {
  return { protocol: ready.protocol, api_version: ready.api_version, build: ready.build, identity,
    challenge: randomUUID(), operation, expected: ready.process ? { process: ready.process, epoch: ready.epoch } : null };
}

export function devServerIdentity(data) {
  return {
    data: path.toNamespacedPath(realpathSync.native(data)),
    config: path.toNamespacedPath(realpathSync.native(path.join(data, 'config.toml'))),
  };
}

export function selected(recordFile, data, port) {
  if (!Number.isInteger(port) || port < 1 || port > 65535 || port === 7685) throw Error('Invalid DEV port');
  const record = JSON.parse(readFileSync(recordFile, 'utf8'));
  if (record.schema !== 3 || Number(record.instance.apiPort) !== port) throw Error('DEV provenance/port mismatch');
  const identity = devServerIdentity(data);
  const executable = record.artifacts.server.path;
  const resources = record.artifacts.resources.path;
  const manifest = JSON.parse(readFileSync(record.artifacts.inventory.path, 'utf8'));
  if (manifest.bootstrap_protocol !== 2 || manifest.server_build !== record.buildSha) throw Error('DEV inventory/build mismatch');
  return { record, identity, executable, resources, build: record.buildSha, port };
}

export async function inspect(target) {
  const request = attachRequest({ protocol: 2, api_version: 3, build: target.build }, target.identity);
  const frames = await helper(target.executable, 'attach', request).done;
  if (frames.length !== 1 || frames[0].challenge !== request.challenge) throw Error('DEV inspect reply mismatch');
  validateReady(frames[0].ready, target);
  return frames[0].ready;
}

async function stop(target, ready) {
  let observed;
  const observation = new Promise(resolve => { observed = resolve; });
  const request = attachRequest(ready, ready.identity);
  const watcher = helper(target.executable, 'observe', request, frame => {
    if (frame.observing) observed(frame.observing);
  });
  try {
    const actual = await Promise.race([observation, watcher.done.then(() => { throw Error('DEV exit observer ended before attachment'); })]);
    if (!isDeepStrictEqual(actual, ready)) throw Error('DEV exit observer attached to a different epoch');
    const shutdown = attachRequest(ready, ready.identity, 'shutdown');
    const frames = await helper(target.executable, 'attach', shutdown).done;
    if (frames.length !== 1 || frames[0].challenge !== shutdown.challenge || !isDeepStrictEqual(frames[0].ready, ready)) {
      throw Error('DEV required-cleanup completion was not confirmed');
    }
    const exited = await watcher.done;
    if (exited.length !== 2 || exited[1].exited !== true) throw Error('DEV pinned process exit was not confirmed');
  } finally { watcher.cancel(); }
}

async function main() {
  const [operation, recordFile, data, portText, receipt] = process.argv.slice(2);
  const target = selected(recordFile, data, Number(portText));
  if (operation === 'inspect') {
    writeFileSync(receipt, JSON.stringify(await inspect(target)));
  } else if (operation === 'stop') {
    await stop(target, JSON.parse(readFileSync(receipt, 'utf8')));
    console.log('DEV server required cleanup and pinned exit confirmed; client and watchers retained.');
  } else if (operation === 'start') {
    const frames = await helper(target.executable, 'launch', {
      instance: { kind: 'development', instance: target.record.instance.name || null, port: target.port },
      resources: target.resources, probe_sink: null,
    }).done;
    if (frames.length !== 1 || frames[0].challenge !== '') throw Error('DEV launch reply mismatch');
    validateReady(frames[0].ready, target);
    const ready = await inspect(target);
    console.log(`DEV server READY build=${ready.build} pid=${ready.process.pid} epoch=${ready.epoch}`);
  } else throw Error('Expected inspect, stop, or start');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
