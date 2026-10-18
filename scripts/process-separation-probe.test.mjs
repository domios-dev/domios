import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync, realpathSync, mkdirSync, readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseProbeArgs, cleanEnvironment, attachRequest } from './process-separation-probe.mjs';
import { assertServerClosure, serverArtifact, workerArtifact, stageDevResources, resolveDevResourceRoot } from './server-artifact.mjs';

test('only explicit disposable process cases are admitted', () => {
  assert.deepEqual(parseProbeArgs(['--case', 'server-only', '--instance', 'split-oracle', '--artifact-receipt', '/receipt']), { case: 'server-only', receipt: '/receipt' });
  for (const args of [[], ['--case', 'production'], ['--case', 'arbitration', '--instance', 'prod', '--artifact-receipt', '/receipt'], ['--case', 'arbitration', '--instance', 'split-oracle', '--port', '7685']]) assert.throws(() => parseProbeArgs(args));
});
test('launch environment discards inherited connection and display selectors', () => {
  const env = cleanEnvironment({ CODETERM_API_URL: 'foreign', CODETERM_API_TOKEN: 'secret', CODETERM_INSTANCE: 'prod', CODETERM_MACHINE_OWNER: 'true', DISPLAY: ':1', WAYLAND_DISPLAY: 'wayland', PATH: '/tools' });
  assert.deepEqual(env, { PATH: '/tools', CODETERM_MACHINE_OWNER: 'false' });
});
test('production closure refuses each GUI edge and an absent server root', () => {
  assertServerClosure(['domios-server v1.14.9', 'tokio v1.0']);
  for (const name of ['tauri', 'tauri-runtime-wry', 'wry', 'web-view', 'webview2-com', 'webkit2gtk', 'codeterm-host-runtime']) assert.throws(() => assertServerClosure(['domios-server v1.14.9', `${name} v2.0`]));
  assert.throws(() => assertServerClosure(['tokio v1.0']));
});
test('worker resource binds the CLI source artifact rather than a same-named GUI binary', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'domios-worker-artifact-unit-'));
  try {
    mkdirSync(path.join(root, 'cli/src'), { recursive: true });
    const source = path.join(root, 'cli/src/main.rs'); writeFileSync(source, 'fixture');
    const foreign = path.join(root, 'gui-main.rs'); writeFileSync(foreign, 'fixture');
    const executable = path.join(root, 'codeterm'); writeFileSync(executable, 'never-executed');
    const message = { reason: 'compiler-artifact', target: { name: 'codeterm', kind: ['bin'], src_path: source }, executable, profile: { test: false } };
    assert.equal(workerArtifact([message], root), realpathSync.native(executable));
    assert.throws(() => workerArtifact([{ ...message, target: { ...message.target, src_path: foreign } }], root));
    assert.throws(() => workerArtifact([message, message], root));
  } finally { rmSync(root, { recursive: true }); }
});
test('artifact selection rejects missing, duplicate and test executable evidence', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'domios-artifact-unit-'));
  try {
    const executable = path.join(root, 'artifact'); writeFileSync(executable, 'not-executed');
    const message = { reason: 'compiler-artifact', target: { name: 'domios-server', kind: ['bin'] }, executable, profile: { test: false } };
    assert.equal(serverArtifact([message]), realpathSync.native(executable));
    for (const messages of [[], [message, message], [{ ...message, profile: { test: true } }]]) assert.throws(() => serverArtifact(messages));
  } finally { rmSync(root, { recursive: true }); }
});
test('control requests bind challenge and exact intended server incarnation', () => {
  const ready = { protocol: 2, api_version: 3, build: 'artifact', identity: { data: '/data', config: '/data/config' }, epoch: 'boot', process: { pid: 7, created: 'born', executable: '/server' } };
  const first = attachRequest(ready), second = attachRequest(ready);
  assert.notEqual(first.challenge, second.challenge);
  assert.deepEqual(first.expected, { process: ready.process, epoch: ready.epoch });
  assert.equal(attachRequest(ready, { operation: 'shutdown' }).operation, 'shutdown');
});

for (const target of ['', 'opaque-target']) test(`DEV stages exact paired artifacts under native resource root: ${target || 'host'}`, async () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'domios-dev-stage-'));
  try {
    const exe = process.platform === 'win32' ? '.exe' : '';
    const artifactRoot = path.join(root, 'target', target, 'debug');
    const resourceRoot = process.platform === 'win32' ? artifactRoot : path.join(artifactRoot, '.domios-dev-runtime/lib/codeterm');
    const server = path.join(artifactRoot, `domios-server${exe}`);
    const client = path.join(artifactRoot, `codeterm${exe}`);
    const cli = path.join(root, `cli/target/debug/codeterm${exe}`);
    const serverSource = path.join(root, 'src-tauri/crates/domios-server/src/main.rs');
    const cliSource = path.join(root, 'cli/src/main.rs');
    for (const file of [server, client, cli, serverSource, cliSource, 'dist/index.html', 'skills/fixture.md', 'LICENSE', 'NOTICE'].map(file => path.isAbsolute(file) ? file : path.join(root, file))) {
      mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, 'opaque-' + path.basename(file));
    }
    const contract = path.join(root, 'src-tauri/crates/codeterm-types/src/server_bootstrap.rs');
    mkdirSync(path.dirname(contract), { recursive: true });
    writeFileSync(contract, 'pub const API_VERSION: u32 = 3;\npub const BOOTSTRAP_PROTOCOL: u32 = 2;\n');
    if (process.platform === 'win32') {
      mkdirSync(path.join(root, 'src-tauri/conpty/x64'), { recursive: true });
      for (const file of ['OpenConsole.exe', 'conpty.dll']) writeFileSync(path.join(root, 'src-tauri/conpty/x64', file), 'fixture');
    }
    const { createHash } = await import('node:crypto');
    const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
    const build = 'a'.repeat(40);
    const cargoLog = path.join(root, 'server.jsonl'), cliLog = path.join(root, 'cli.jsonl'), treeLog = path.join(root, 'tree.txt');
    const scriptReceipt = { reason: 'build-script-executed', package_id: 'domios-server', env: [['BUILD_SHA', build], ['SERVER_CLI_SHA256', digest(cli)]] };
    const artifact = { reason: 'compiler-artifact', target: { name: 'domios-server', kind: ['bin'], src_path: serverSource }, executable: server, profile: { test: false }, features: ['embeddings'] };
    const finish = { reason: 'build-finished', success: true };
    writeFileSync(cargoLog, [scriptReceipt, artifact, finish].map(JSON.stringify).join('\n'));
    writeFileSync(cliLog, [{ ...artifact, target: { name: 'codeterm', kind: ['bin'], src_path: cliSource }, executable: cli }, finish].map(JSON.stringify).join('\n'));
    writeFileSync(treeLog, 'domios-server v0.0.0\ntokio v1.0\n');
    const input = { cargoLog, cliLog, treeLog, repo: root, resourceRoot, client, build };
    const result = await stageDevResources(input);
    assert.equal(result.manifest.bootstrap_protocol, 2);
    assert.equal(result.manifest.server_sha256, digest(server));
    assert.deepEqual(result.manifest.client_identity, { kind: 'sha256', sha256: digest(client) });
    assert.equal(digest(process.platform === 'win32' ? client : path.resolve(resourceRoot, '../../bin', path.basename(client))), digest(client));
    assert.equal(readFileSync(server, 'utf8'), 'opaque-' + path.basename(server));
    await assert.rejects(stageDevResources({ ...input, resourceRoot: path.join(root, 'untrusted-root') }), /resource root/);
    assert.equal(result.worker_cli_sha256, digest(cli));
    assert.equal(readFileSync(path.join(resourceRoot, result.manifest.server_executable), 'utf8'), readFileSync(server, 'utf8'));
    const inventory = readFileSync(path.join(resourceRoot, 'server-component.json'));
    const foreignClient = path.join(root, 'foreign', path.basename(client));
    mkdirSync(path.dirname(foreignClient)); writeFileSync(foreignClient, 'foreign');
    await assert.rejects(stageDevResources({ ...input, client: foreignClient }), /resource root/);
    const wrongName = path.join(artifactRoot, 'foreign-client');
    writeFileSync(wrongName, 'foreign');
    await assert.rejects(stageDevResources({ ...input, client: wrongName }), /client artifact filename/);
    assert.deepEqual(readFileSync(path.join(resourceRoot, 'server-component.json')), inventory);
    writeFileSync(cargoLog, [{ ...scriptReceipt, env: [['BUILD_SHA', build], ['SERVER_CLI_SHA256', 'b'.repeat(64)]] }, artifact, finish].map(JSON.stringify).join('\n'));
    await assert.rejects(stageDevResources(input), /CLI binding/);
    assert.deepEqual(readFileSync(path.join(resourceRoot, 'server-component.json')), inventory);
    writeFileSync(treeLog, 'domios-server v0.0.0\ntauri v2.0\n');
    await assert.rejects(stageDevResources(input), /forbidden/);
  } finally { rmSync(root, { recursive: true }); }
});


test('DEV root confinement accepts resolved SUBST spelling, not junctions or foreign roots', () => {
  const paths = path.win32;
  const expected = 'C:/physical/target/debug';
  const physical = paths.resolve(expected);
  const prepared = [];
  let junction = false;
  const boundary = {
    paths,
    stat: file => {
      if (file.endsWith('missing')) throw Object.assign(new Error('absent'), { code: 'ENOENT' });
      return { isDirectory: () => true, isSymbolicLink: () => junction && file === paths.resolve('X:/debug') };
    },
    canonical: file => file.startsWith('X:') ? paths.resolve('C:/physical/target', paths.relative('X:/', file)) : file,
    prepare: file => prepared.push(file),
  };
  assert.equal(resolveDevResourceRoot('X:/debug', physical, boundary), physical);
  assert.deepEqual(prepared, [physical]);
  assert.throws(() => resolveDevResourceRoot('X:/foreign', physical, boundary), /resource root/);
  junction = true;
  assert.throws(() => resolveDevResourceRoot('X:/debug', physical, boundary), /symlink or junction/);
  junction = false;
  const derived = paths.join(physical, 'missing');
  assert.equal(resolveDevResourceRoot('X:/debug/missing', derived, boundary), derived);
  assert.deepEqual(prepared, [physical, derived]);
});
