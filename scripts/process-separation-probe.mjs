// WHAT: exercise real isolated server lifetime and first-launch arbitration.
// WHY: source models cannot prove OS ownership, private IPC or PTY survival.
// ORACLE: authenticated same-epoch readiness and exact process-exit handles, with one initialization audit.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomUUID, createHash } from 'node:crypto';
import { once } from 'node:events';
import { existsSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, watch, lstatSync, unlinkSync } from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertServerClosure, SERVER_FEATURES } from './server-artifact.mjs';

const deadline = () => AbortSignal.timeout(45000);
export function parseProbeArgs(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!['--case', '--instance', '--artifact-receipt'].includes(argv[i]) || !argv[i + 1] || options[argv[i]]) throw new Error('Expected --case server-only|arbitration --instance split-oracle');
    options[argv[i]] = argv[i + 1];
  }
  if (!['server-only', 'arbitration'].includes(options['--case']) || options['--instance'] !== 'split-oracle' || !options['--artifact-receipt']) throw new Error('Only the explicit disposable split-oracle instance is permitted');
  return { case: options['--case'], receipt: options['--artifact-receipt'] };
}
export function cleanEnvironment(environment) {
  const result = Object.fromEntries(Object.entries(environment).filter(([key]) => !key.startsWith('CODETERM_') && !['DISPLAY', 'WAYLAND_DISPLAY', 'TAURI_DEV_HOST'].includes(key)));
  result.CODETERM_MACHINE_OWNER = 'false';
  return result;
}
export function attachRequest(ready, overrides = {}) {
  return { protocol: ready.protocol, api_version: ready.api_version, build: ready.build, identity: ready.identity, challenge: randomUUID(), operation: 'inspect', expected: { process: ready.process, epoch: ready.epoch }, ...overrides };
}
function broker(executable, command, request, { hold = false, env = cleanEnvironment(process.env) } = {}) {
  const child = spawn(executable, [command, ...(hold ? ['--hold'] : [])], { env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  const exit = once(child, 'exit', { signal: deadline() });
  let error = '';
  child.stderr.on('data', bytes => { error = (error + bytes.toString()).slice(-4096); });
  let pending = '', rows = [], receivers = [];
  child.stdout.on('data', bytes => {
    pending += bytes.toString('utf8');
    if (pending.length > 32768) { child.kill(); return; }
    for (let at; (at = pending.indexOf('\n')) >= 0;) {
      const line = pending.slice(0, at); pending = pending.slice(at + 1);
      let value;
      try { value = JSON.parse(line); } catch { value = { invalid_frame: true }; }
      if (receivers.length) receivers.shift()(value); else rows.push(value);
    }
  });
  child.stdin.write(`${JSON.stringify(request)}\n`);
  if (!hold) child.stdin.end();
  const line = async () => {
    if (rows.length) return rows.shift();
    const signal = deadline();
    return Promise.race([
      new Promise(resolve => receivers.push(resolve)),
      exit.then(() => { throw new Error('Broker exited before framed acknowledgement'); }),
      new Promise((_, reject) => signal.addEventListener('abort', () => reject(new Error('Broker acknowledgement deadline')), { once: true })),
    ]);
  };
  return { child, exit, line, error: () => error };
}
async function freePort() {
  const server = net.createServer(); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  const port = server.address().port; await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())); return port;
}
async function api(reply, endpoint, body) {
  const response = await fetch(`http://${reply.ready.api_address}${endpoint}`, { method: body ? 'POST' : 'GET', headers: { Authorization: `Bearer ${reply.api_token}`, 'Content-Type': 'application/json' }, ...(body ? { body: JSON.stringify(body) } : {}), signal: deadline() });
  assert.equal(response.ok, true, `Isolated API refused ${endpoint}: ${response.status}`);
  return response.json();
}
async function inspect(executable, reply, overrides) {
  const request = attachRequest(reply.ready, overrides);
  const helper = broker(executable, 'attach', request);
  const result = await helper.line();
  assert.equal((await helper.exit)[0], 0);
  assert.equal(result.challenge, request.challenge);
  assert.deepEqual(result.ready, reply.ready);
  return result;
}
async function negative(executable, reply, overrides) {
  const helper = broker(executable, 'attach', attachRequest(reply.ready, overrides));
  assert.notEqual((await helper.exit)[0], 0, 'Invalid attach must refuse');
}
function socketName(name) {
  if (process.platform === 'win32') return `\\\\.\\pipe\\${name}`;
  return process.platform === 'linux' ? `\0${name}` : `/tmp/${name}`;
}
async function socketProof(ready) {
  const name = ready.socket_address.split(/[\\/]/).at(-1);
  const socket = net.createConnection(socketName(name));
  let pending = '';
  const signal = deadline();
  const data = new Promise((resolve, reject) => {
    socket.on('data', bytes => {
      pending += bytes.toString('utf8');
      if (pending.length > 32768) reject(new Error('Socket acknowledgement exceeds bound'));
      if (pending.includes('\n')) { try { resolve(JSON.parse(pending.slice(0, pending.indexOf('\n')))); } catch { reject(new Error('Invalid socket acknowledgement')); } }
    });
    socket.once('error', reject);
    socket.once('end', () => reject(new Error('Socket ended before acknowledgement')));
    signal.addEventListener('abort', () => reject(new Error('Socket acknowledgement deadline')), { once: true });
  });
  await once(socket, 'connect', { signal: deadline() });
  socket.write(`${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'system.identify', protocol_version: 6 })}\n`);
  try { assert.ok((await data).result?.version); }
  finally { socket.destroy(); }
}
async function maintenanceEvent(data, receipt) {
  const file = path.join(data, 'process-worker.jsonl');
  const signal = deadline();
  let watcher;
  try {
    const event = await new Promise((resolve, reject) => {
      const read = () => {
        try {
          const content = readFileSync(file, 'utf8');
          if (content.length > 32768) throw new Error('Worker observation exceeds bound');
          if (content.includes('\n')) resolve(JSON.parse(content.slice(0, content.indexOf('\n'))));
        } catch (error) { reject(error); }
      };
      watcher = watch(data, (_, name) => { if (name?.toString() === 'process-worker.jsonl') read(); });
      watcher.once('error', reject);
      signal.addEventListener('abort', () => reject(new Error('Actual maintenance observation deadline')), { once: true });
      read();
    });
    assert.equal(event.status, 'verified', 'Actual maintenance child provenance unavailable; native row fails');
    assert.equal(event.resource_sha256, receipt.worker_cli_sha256);
    assert.equal(event.selected, event.process.executable);
    assert.ok(event.process.pid > 0 && event.process.created);
    return event;
  } finally { watcher?.close(); }
}
async function foreignExecutableRefuses(ready) {
  const socket = net.createConnection(socketName(ready.bootstrap_address));
  const closed = once(socket, 'close', { signal: deadline() });
  let received = 0;
  socket.on('data', bytes => { received += bytes.length; });
  socket.on('error', () => {});
  await closed;
  assert.equal(received, 0, 'Foreign executable must not receive a bootstrap credential');
}
async function run(options) {
  const receipt = JSON.parse(readFileSync(options.receipt, 'utf8'));
  assertServerClosure(receipt.dependency_packages);
  assert.deepEqual(receipt.features, SERVER_FEATURES);
  assert.equal(receipt.profile, 'dev');
  assert.equal(createHash('sha256').update(readFileSync(receipt.executable)).digest('hex'), receipt.executable_sha256);
  const instance = `split-oracle-${randomUUID().slice(0, 8)}`;
  const data = path.join(realpathSync.native(os.homedir()), `.codeterm-dev-${instance}`);
  assert.equal(existsSync(data), false, 'A fresh disposable identity is required');
  const scratch = mkdtempSync(path.join(os.tmpdir(), 'domios-process-probe-'));
  const alias = path.join(scratch, 'home-alias'); symlinkSync(os.homedir(), alias, process.platform === 'win32' ? 'junction' : 'dir');
  const port = await freePort();
  const launch = { instance: { kind: 'development', instance, port }, resources: receipt.resources, probe_sink: path.join(data, 'process-init.jsonl') };
  const launchers = []; let reply; let observation; let stopped = false; let result;
  try {
    const rejectedInstance = `${instance}-missing`;
    const rejectedData = path.join(realpathSync.native(os.homedir()), `.codeterm-dev-${rejectedInstance}`);
    assert.equal(existsSync(rejectedData), false);
    const missing = broker(receipt.executable, 'launch', { ...launch, resources: scratch, instance: { ...launch.instance, instance: rejectedInstance }, probe_sink: path.join(rejectedData, 'process-init.jsonl') });
    assert.notEqual((await missing.exit)[0], 0, 'Missing exact CLI resource must refuse before service initialization');
    assert.equal(existsSync(rejectedData), false, 'Missing CLI must not reach service initialization or legacy CLI installation');
    const first = broker(receipt.executable, 'launch', launch, { hold: true }); launchers.push(first);
    if (options.case === 'arbitration') {
      const aliasData = path.join(alias, `.codeterm-dev-${instance}`);
      launchers.push(broker(receipt.executable, 'launch', { ...launch, probe_identity_alias: { data: aliasData, config: path.join(aliasData, 'config.toml') } }, { hold: true }));
      const results = await Promise.all(launchers.map(async helper => {
        try { return { helper, reply: await helper.line() }; } catch { return { helper, exit: await helper.exit }; }
      }));
      const owners = results.filter(result => result.reply?.ready);
      assert.equal(owners.length, 1, 'Concurrent aliases must yield one exact owner');
      reply = owners[0].reply;
      assert.notEqual(results.find(result => !result.reply).exit[0], 0);
    } else { reply = await first.line(); }
    assert.equal(reply.ready.build, receipt.build);
    assert.equal(realpathSync.native(reply.ready.process.executable), realpathSync.native(receipt.executable));
    assert.equal(realpathSync.native(reply.ready.identity.data), realpathSync.native(data));
    assert.notEqual(reply.ready.process.pid, process.pid);
    assert.equal(reply.ready.api_version, 3);
    assert.equal(reply.ready.api_address, `127.0.0.1:${port}`);
    const auditBefore = readFileSync(launch.probe_sink, 'utf8');
    const audit = auditBefore.trim().split('\n').map(JSON.parse);
    assert.equal(new Set(audit.map(row => row.pid)).size, 1);
    assert.deepEqual(audit.map(row => row.phase), ['mesh', 'crash', 'config-credential-hook-state']);
    assert.equal(audit[0].pid, reply.ready.process.pid);
    const maintenance = await maintenanceEvent(data, receipt);
    assert.notEqual(maintenance.process.pid, reply.ready.process.pid);
    const observationRequest = attachRequest(reply.ready);
    observation = broker(receipt.executable, 'observe', observationRequest);
    assert.deepEqual((await observation.line()).observing, reply.ready);
    const workspace = await api(reply, '/api/workspaces', { name: 'ProcessProbe', cwd: scratch });
    const tab = await api(reply, '/api/tabs', { workspace_id: workspace.id, cwd: scratch, title: 'Probe', eager: true });
    assert.ok(tab.pty_id, 'Disposable session must own a real PTY before launcher termination');
    for (const helper of launchers) {
      if (helper.child.exitCode === null) helper.child.kill();
      await helper.exit;
    }
    const reattached = await inspect(receipt.executable, reply);
    await socketProof(reply.ready);
    await foreignExecutableRefuses(reply.ready);
    const tabs = await api(reattached, '/api/tabs?local_only=true');
    const same = tabs.find(row => row.id === tab.id);
    assert.ok(same, 'Session must survive launcher exit');
    assert.equal(same.pty_id, tab.pty_id);
    if (options.case === 'arbitration') {
      await negative(receipt.executable, reply, { protocol: reply.ready.protocol + 1 });
      await negative(receipt.executable, reply, { build: `${reply.ready.build}-foreign` });
      await negative(receipt.executable, reply, { expected: { process: reply.ready.process, epoch: 'stale-epoch' } });
      await negative(receipt.executable, reply, { expected: { process: { ...reply.ready.process, created: 'reused-pid' }, epoch: reply.ready.epoch } });
      await negative(receipt.executable, reply, { identity: { ...reply.ready.identity, data: path.join(scratch, 'foreign') } });
      assert.equal(readFileSync(launch.probe_sink, 'utf8'), auditBefore, 'Rejected contenders must not initialize again');
      const occupied = net.createServer(); occupied.listen(0, '127.0.0.1'); await once(occupied, 'listening');
      const foreignInstance = `${instance}-occupied`;
      const foreignData = path.join(realpathSync.native(os.homedir()), `.codeterm-dev-${foreignInstance}`);
      assert.equal(existsSync(foreignData), false);
      try {
        const conflict = broker(receipt.executable, 'launch', { ...launch, instance: { kind: 'development', instance: foreignInstance, port: occupied.address().port }, probe_sink: path.join(foreignData, 'process-init.jsonl') });
        assert.notEqual((await conflict.exit)[0], 0, 'Foreign listener cannot be claimed as ready');
        assert.equal(occupied.listening, true, 'Foreign listener must retain its own OS handle');
      } finally { await new Promise(resolve => occupied.close(resolve)); rmSync(foreignData, { recursive: true, force: true }); }
      const forbidden = broker(receipt.executable, 'launch', { ...launch, instance: { kind: 'production' } });
      assert.notEqual((await forbidden.exit)[0], 0, 'DEV artifact must refuse PROD selection');
    }
    await inspect(receipt.executable, reply, { operation: 'shutdown' });
    assert.deepEqual(await observation.line(), { exited: true });
    assert.equal((await observation.exit)[0], 0);
    stopped = true;
    result = { status: 'PASS', case: options.case, platform: process.platform, target: receipt.target, build: receipt.build, ready: reply.ready, maintenance, alias: { source: alias, target: realpathSync.native(alias), kind: process.platform === 'win32' ? 'junction' : 'symlink' }, missing_resource_refused_before_initialization: true, initialization_phases: audit.length, frontend_acknowledgements: 0, exact_exit_observed: true };
  } finally {
    if (reply && !stopped) {
      try { await inspect(receipt.executable, reply, { operation: 'shutdown' }); if (observation) { await observation.line(); await observation.exit; } stopped = true; } catch { /* Retain data if exact cleanup cannot be proven. */ }
    }
    for (const helper of launchers) if (helper.child.exitCode === null) { helper.child.kill(); await helper.exit; }
    assert.equal(path.dirname(alias), scratch);
    assert.equal(lstatSync(alias).isSymbolicLink(), true);
    assert.equal(realpathSync.native(alias), realpathSync.native(os.homedir()));
    unlinkSync(alias);
    if (stopped) rmSync(data, { recursive: true });
    rmSync(scratch, { recursive: true });
    if (reply && !stopped) throw new Error('Exact server cleanup unproven; disposable data retained');
  }
  console.log(JSON.stringify(result));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === '--validate') {
    parseProbeArgs([...process.argv.slice(3), '--artifact-receipt', 'validation-only']);
  } else run(parseProbeArgs(process.argv.slice(2))).catch(error => { console.error(`process-probe FAILED: ${error.message}`); process.exitCode = 1; });
}
