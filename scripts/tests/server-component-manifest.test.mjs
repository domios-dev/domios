import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { MANIFEST_RELATIVE_PATH, createManifest, validateManifest } from '../package-server-component-manifest.mjs';

const REPOSITORY_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const GENERATOR_PATH = path.join(REPOSITORY_ROOT, 'scripts/package-server-component-manifest.mjs');
const FIXTURE_PATH = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures/server-component-manifest.json');
const SERVER_RELATIVE_PATH = 'domios-server/domios-server';
const SERVER_BYTES = Buffer.from('server-fixture-bytes-v1');
const CLIENT_BYTES = Buffer.from('client-fixture-bytes-v1');

const fixture = JSON.parse(await readFile(FIXTURE_PATH, 'utf8'));

async function makeStage() {
  const root = await realpath(await mkdtemp(path.join(os.tmpdir(), 'server-component-manifest-')));
  const serverFile = path.join(root, 'target/domios-package', ...SERVER_RELATIVE_PATH.split('/'));
  const clientFile = path.join(root, 'native-client/domios');
  await mkdir(path.dirname(serverFile), { recursive: true });
  await mkdir(path.dirname(clientFile), { recursive: true });
  await writeFile(serverFile, SERVER_BYTES);
  await writeFile(clientFile, CLIENT_BYTES);
  return { root, serverFile, clientFile, outputFile: path.join(root, MANIFEST_RELATIVE_PATH) };
}

function runGenerator(stage, overrides = {}) {
  const options = {
    serverFile: stage.serverFile,
    clientFile: stage.clientFile,
    serverBuild: 'fixture-server-build',
    serverExecutable: SERVER_RELATIVE_PATH,
    apiVersion: '3',
    bootstrapProtocol: '2',
    ...overrides,
  };
  const args = [
    GENERATOR_PATH,
    '--server-file', options.serverFile,
    '--client-file', options.clientFile,
    '--server-build', options.serverBuild,
    '--server-executable', options.serverExecutable,
    '--api-version', options.apiVersion,
    '--bootstrap-protocol', options.bootstrapProtocol,
  ];
  return spawnSync(process.execPath, args, { cwd: stage.root, encoding: 'utf8' });
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function isContained(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative !== '' && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}

test('emits the exact v2 SHA identity from independently hashed staged artifacts', async t => {
  const stage = await makeStage();
  t.after(() => rm(stage.root, { recursive: true, force: true }));

  const result = runGenerator(stage);
  assert.equal(result.status, 0, result.stderr);
  const manifest = JSON.parse(await readFile(stage.outputFile, 'utf8'));
  assert.deepEqual(Object.keys(manifest), Object.keys(fixture));
  assert.deepEqual(manifest, fixture);
  assert.equal(manifest.server_sha256, sha256(await readFile(stage.serverFile)));
  assert.equal(manifest.client_identity.sha256, sha256(await readFile(stage.clientFile)));

  const resourceRoot = path.join(stage.root, 'installed/resources');
  const resolvedServer = path.resolve(resourceRoot, ...manifest.server_executable.split('/'));
  assert.ok(isContained(resourceRoot, resolvedServer));
  assert.notEqual(path.resolve(stage.outputFile), resolvedServer);
  assert.notEqual(path.resolve(stage.outputFile), stage.serverFile);
  assert.notEqual(path.resolve(stage.outputFile), stage.clientFile);
  assert.deepEqual(validateManifest(manifest), manifest);
});
test('changing one staged artifact changes only its matching digest', async t => {
  const stage = await makeStage();
  t.after(() => rm(stage.root, { recursive: true, force: true }));

  assert.equal(runGenerator(stage).status, 0);
  const original = JSON.parse(await readFile(stage.outputFile, 'utf8'));

  await writeFile(stage.serverFile, 'changed-server-fixture-v1');
  assert.equal(runGenerator(stage).status, 0);
  const serverChanged = JSON.parse(await readFile(stage.outputFile, 'utf8'));
  assert.notEqual(serverChanged.server_sha256, original.server_sha256);
  assert.deepEqual(serverChanged.client_identity, original.client_identity);
  assert.deepEqual({ ...serverChanged, server_sha256: original.server_sha256 }, original);

  await writeFile(stage.serverFile, SERVER_BYTES);
  await writeFile(stage.clientFile, 'changed-client-fixture-v1');
  assert.equal(runGenerator(stage).status, 0);
  const clientChanged = JSON.parse(await readFile(stage.outputFile, 'utf8'));
  assert.equal(clientChanged.server_sha256, original.server_sha256);
  assert.notEqual(clientChanged.client_identity.sha256, original.client_identity.sha256);
  assert.deepEqual({ ...clientChanged, client_identity: original.client_identity }, original);
});

test('macOS designated requirement has the fixed nested server path without a raw client hash', async t => {
  const stage = await makeStage();
  t.after(() => rm(stage.root, { recursive: true, force: true }));
  const teamId = 'JMMWQ9LWF7';
  await createManifest({ cwd: stage.root, serverFile: stage.serverFile,
    serverBuild: 'fixture-server-build', serverExecutable: '../MacOS/domios-server',
    clientIdentity: { kind: 'macos_designated_requirement',
      requirement: `anchor apple generic and identifier "com.domios.app" and certificate leaf[subject.OU] = "${teamId}"`,
      team_id: teamId, identifier: 'com.domios.app' }, apiVersion: 3, bootstrapProtocol: 2 });
  const manifest = JSON.parse(await readFile(stage.outputFile, 'utf8'));
  assert.equal(manifest.schema, 2);
  assert.equal(manifest.server_executable, '../MacOS/domios-server');
  assert.equal(manifest.server_sha256, sha256(SERVER_BYTES));
  assert.equal(Object.hasOwn(manifest, 'client_sha256'), false);
  assert.throws(() => validateManifest({ ...manifest, server_executable: 'domios-server/domios-server' }));
  assert.throws(() => validateManifest({ ...manifest, client_identity: { kind: 'macos_designated_requirement',
    requirement: 'identifier "com.domios.app"', team_id: teamId, identifier: 'com.domios.app' } }));
});

test('missing staged artifacts refuse generation and remove any stale manifest', async t => {
  for (const missing of ['server', 'client']) {
    const stage = await makeStage();
    t.after(() => rm(stage.root, { recursive: true, force: true }));
    assert.equal(runGenerator(stage).status, 0);
    await writeFile(stage.outputFile, 'stale manifest');
    await rm(missing === 'server' ? stage.serverFile : stage.clientFile);

    const result = runGenerator(stage);
    assert.notEqual(result.status, 0, result.stdout);
    await assert.rejects(readFile(stage.outputFile));
  }
});

test('refuses absolute and escaping server resource paths', async t => {
  const stage = await makeStage();
  t.after(() => rm(stage.root, { recursive: true, force: true }));

  for (const serverExecutable of [
    '../domios-server',
    'domios-server/../../outside',
    '/absolute/domios-server',
    'C:/absolute/domios-server.exe',
  ]) {
    const result = runGenerator(stage, { serverExecutable });
    assert.notEqual(result.status, 0, serverExecutable);
    await assert.rejects(readFile(stage.outputFile));
  }
});

test('refuses malformed versions, digests, and unsupported protocol values', async t => {
  const stage = await makeStage();
  t.after(() => rm(stage.root, { recursive: true, force: true }));

  for (const apiVersion of ['0', '3.0', '4294967296']) {
    assert.notEqual(runGenerator(stage, { apiVersion }).status, 0, apiVersion);
  }
  assert.notEqual(runGenerator(stage, { serverBuild: '' }).status, 0);
  assert.notEqual(runGenerator(stage, { bootstrapProtocol: '1' }).status, 0);

  assert.throws(() => validateManifest({ ...fixture, server_sha256: 'not-a-digest' }));
  assert.throws(() => validateManifest({ ...fixture, client_identity: { kind: 'sha256', sha256: 'A'.repeat(64) } }));
  assert.throws(() => validateManifest({ ...fixture, api_version: '3' }));
  assert.throws(() => validateManifest({ ...fixture, schema: 1 }));
  assert.throws(() => validateManifest({ ...fixture, unsupported: true }));
});
