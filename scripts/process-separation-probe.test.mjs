import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync, realpathSync, mkdirSync, readFileSync, existsSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseProbeArgs, cleanEnvironment, attachRequest, assertDisposableHome, assertSafePort,
  clientArtifact, clientBuildTarget, clientEnvironmentContract, pairPlatformSupport } from './process-separation-probe.mjs';
import { assertServerClosure, serverArtifact, workerArtifact, stageDevResources, resolveDevResourceRoot } from './server-artifact.mjs';

const legacy = ['code', 'term'].join('');

test('only explicit disposable process cases are admitted', () => {
  assert.deepEqual(parseProbeArgs(['--case', 'server-only', '--instance', 'split-oracle', '--artifact-receipt', '/receipt']), { case: 'server-only', receipt: '/receipt', clientReceipt: null });
  assert.deepEqual(parseProbeArgs(['--case', 'pair-journey', '--instance', 'split-oracle']), { case: 'pair-journey', receipt: null, clientReceipt: null });
  for (const args of [[], ['--case', 'production'], ['--case', 'arbitration', '--instance', 'prod', '--artifact-receipt', '/receipt'], ['--case', 'arbitration', '--instance', 'split-oracle', '--port', '7685']]) assert.throws(() => parseProbeArgs(args));
});
test('launch environment discards inherited identity and display selectors into a disposable home', (t) => {
  const home = mkdtempSync(path.join(realpathSync.native(os.tmpdir()), 'probe-home-'));
  t.after(() => rmSync(home, { recursive: true, force: true }));
  const runSource = readFileSync(path.resolve('src-tauri/src/run.rs'), 'utf8');
  const prefix = clientEnvironmentContract(runSource).instance.replace(/(?:_DEV)?_INSTANCE$/, '').toUpperCase();
  const legacyPrefix = legacy.toUpperCase();
  const ownerEnv = readFileSync(path.resolve('src-tauri/crates/domios-server/src/launch.rs'), 'utf8').match(/"([A-Z][A-Z0-9_]*_MACHINE_OWNER)"/)?.[1];
  assert.ok(ownerEnv);
  const env = cleanEnvironment({ [`${prefix}_API_URL`]: 'foreign', [`${prefix}_API_TOKEN`]: 'secret', [`${prefix}_INSTANCE`]: 'prod',
    [`${legacyPrefix}_API_URL`]: 'legacy', [`${legacyPrefix}_MACHINE_OWNER`]: 'true', [ownerEnv]: 'true',
    DISPLAY: ':1', WAYLAND_DISPLAY: 'wayland', PATH: '/tools' }, home);
  assert.deepEqual(env, { PATH: '/tools', [ownerEnv]: 'false', HOME: home, USERPROFILE: home,
    APPDATA: path.join(home, 'AppData/Roaming'), LOCALAPPDATA: path.join(home, 'AppData/Local'), TMP: path.join(home, 'tmp'),
    TEMP: path.join(home, 'tmp'), TMPDIR: path.join(home, 'tmp'), XDG_CONFIG_HOME: path.join(home, '.config'),
    XDG_DATA_HOME: path.join(home, '.local/share'), XDG_CACHE_HOME: path.join(home, '.cache') });
});
test('protected ports, unnamed data roots and the owner data tree are refused', (t) => {
  assert.equal(assertSafePort(7711), 7711);
  for (const port of [7685, 7686, 0, 65536, Number.NaN]) assert.throws(() => assertSafePort(port));
  assert.throws(() => cleanEnvironment({}, os.homedir()), /disposable temp root/);
  const temp = mkdtempSync(path.join(realpathSync.native(os.tmpdir()), 'probe-home-boundary-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const owner = path.join(temp, 'owner');
  mkdirSync(owner);
  const safe = path.join(temp, 'named-instance', 'home');
  mkdirSync(safe, { recursive: true });
  assert.equal(assertDisposableHome(safe, owner, temp), path.resolve(safe));
  assert.throws(() => assertDisposableHome(owner, owner, temp));
  assert.throws(() => assertDisposableHome(path.join(owner, `.${legacy}`), owner, temp));
  assert.throws(() => assertDisposableHome(path.join(owner, `.${legacy}-dev`), owner, temp));
  assert.throws(() => assertDisposableHome(path.join(owner, `.${legacy}`, 'instance'), owner, temp));
  assert.throws(() => assertDisposableHome(path.join(realpathSync.native(os.tmpdir()), 'outside-probe-home'), owner, temp));
});
test('native binary and environment names come from their source contracts', (t) => {
  const target = clientBuildTarget('[package]\nname = "native-app"\ndefault-run = "native-window"\n\n[[bin]]\nname = "native-window"\npath = "src/main.rs"\n');
  assert.deepEqual(target, { package: 'native-app', binary: 'native-window' });
  assert.throws(() => clientBuildTarget('[package]\nversion = "0.0.0"\n'));
  assert.deepEqual(clientEnvironmentContract('std::env::var_os("SAMPLE_DEV_INSTANCE"); std::env::var_os("SAMPLE_API_PORT");'),
    { instance: 'SAMPLE_DEV_INSTANCE', port: 'SAMPLE_API_PORT' });

  const root = mkdtempSync(path.join(realpathSync.native(os.tmpdir()), 'native-client-artifact-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const source = path.join(root, 'src/main.rs');
  const executable = path.join(root, `native-window${process.platform === 'win32' ? '.exe' : ''}`);
  mkdirSync(path.dirname(source), { recursive: true });
  writeFileSync(source, 'source'); writeFileSync(executable, 'artifact');
  const message = { reason: 'compiler-artifact', target: { name: 'native-window', kind: ['bin'], src_path: source }, executable, profile: { test: false } };
  assert.equal(clientArtifact([message], source), message);
  assert.throws(() => clientArtifact([], source));
  assert.throws(() => clientArtifact([message, message], source));
  assert.throws(() => clientArtifact([{ ...message, profile: { test: true } }], source));
});
test('platforms without a native window driver produce an explicit unverified row reason', () => {
  assert.deepEqual(pairPlatformSupport('win32'), { supported: true, method: 'WinEvent close/reopen hooks and UI Automation tray-count evidence' });
  for (const platform of ['darwin', 'linux', 'freebsd']) {
    const support = pairPlatformSupport(platform);
    assert.equal(support.supported, false);
    assert.match(support.reason, new RegExp(platform));
  }
});
test('production closure refuses each GUI edge and an absent server root', () => {
  assertServerClosure(['domios-server v1.14.9', 'tokio v1.0']);
  for (const name of ['tauri', 'tauri-runtime-wry', 'wry', 'web-view', 'webview2-com', 'webkit2gtk', `${legacy}-client-shell`]) assert.throws(() => assertServerClosure(['domios-server v1.14.9', `${name} v2.0`]));
  assert.throws(() => assertServerClosure(['tokio v1.0']));
});
test('worker resource binds the CLI source artifact rather than a same-named GUI binary', () => {
  const root = mkdtempSync(path.join(realpathSync.native(os.tmpdir()), 'domios-worker-artifact-unit-'));
  try {
    mkdirSync(path.join(root, 'cli/src'), { recursive: true });
    const source = path.join(root, 'cli/src/main.rs'); writeFileSync(source, 'fixture');
    const foreign = path.join(root, 'gui-main.rs'); writeFileSync(foreign, 'fixture');
    const executable = path.join(root, legacy); writeFileSync(executable, 'never-executed');
    const message = { reason: 'compiler-artifact', target: { name: legacy, kind: ['bin'], src_path: source }, executable, profile: { test: false } };
    assert.equal(workerArtifact([message], root), realpathSync.native(executable));
    assert.throws(() => workerArtifact([{ ...message, target: { ...message.target, src_path: foreign } }], root));
    assert.throws(() => workerArtifact([message, message], root));
  } finally { rmSync(root, { recursive: true }); }
});
test('artifact selection rejects missing, duplicate and test executable evidence', () => {
  const root = mkdtempSync(path.join(realpathSync.native(os.tmpdir()), 'domios-artifact-unit-'));
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
  const root = mkdtempSync(path.join(realpathSync.native(os.tmpdir()), 'domios-dev-stage-'));
  try {
    const exe = process.platform === 'win32' ? '.exe' : '';
    const artifactRoot = path.join(root, 'target', target, 'debug');
    const resourceRoot = process.platform === 'win32' ? artifactRoot : path.join(artifactRoot, `.domios-dev-runtime/lib/${legacy}`);
    const server = path.join(artifactRoot, `domios-server${exe}`);
    const client = path.join(artifactRoot, `${legacy}${exe}`);
    const cli = path.join(root, `cli/target/debug/${legacy}${exe}`);
    const serverSource = path.join(root, 'src-tauri/crates/domios-server/src/main.rs');
    const cliSource = path.join(root, 'cli/src/main.rs');
    for (const file of [server, client, cli, serverSource, cliSource, 'dist/index.html', 'skills/fixture.md', 'LICENSE', 'NOTICE'].map(file => path.isAbsolute(file) ? file : path.join(root, file))) {
      mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, 'opaque-' + path.basename(file));
    }
    const contract = path.join(root, `src-tauri/crates/${legacy}-types/src/server_bootstrap.rs`);
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
    writeFileSync(cliLog, [{ ...artifact, target: { name: legacy, kind: ['bin'], src_path: cliSource }, executable: cli }, finish].map(JSON.stringify).join('\n'));
    writeFileSync(treeLog, 'domios-server v0.0.0\ntokio v1.0\n');
    const input = { cargoLog, cliLog, treeLog, repo: root, resourceRoot, client, build };
    const staleChunk = path.join(resourceRoot, 'dist/assets/old-build.js');
    mkdirSync(path.dirname(staleChunk), { recursive: true });
    writeFileSync(staleChunk, 'obsolete bundle input');
    const result = await stageDevResources(input);
    assert.equal(existsSync(staleChunk), false);
    assert.equal(readFileSync(path.join(resourceRoot, 'dist/index.html'), 'utf8'), 'opaque-index.html');
    assert.equal(result.manifest.bootstrap_protocol, 2);
    assert.equal(result.manifest.server_sha256, digest(server));
    assert.deepEqual(result.manifest.client_identity, { kind: 'sha256', sha256: digest(client) });
    assert.equal(digest(process.platform === 'win32' ? path.join(resourceRoot, 'domios-dev-app.exe') : path.resolve(resourceRoot, '../../bin/domios-dev-app')), digest(client));
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
