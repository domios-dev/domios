import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { lstat, mkdtemp, mkdir, readFile, readdir, rm, unlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  HEADLESS_TARGETS,
  headlessAssetNames,
  packageHeadless,
  readHeadlessArchive,
  validateHeadlessStage,
} from '../headless-release-package.mjs';

const identity = {
  tag: 'v1.2.3',
  version: '1.2.3',
  sourceSha: 'a'.repeat(40),
};

function hash(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function text(value) {
  return Buffer.from(value);
}

async function writePayload(root, relativePath, bytes) {
  const absolutePath = path.join(root, ...relativePath.split('/'));
  await mkdir(path.dirname(absolutePath), { recursive: true });
  await writeFile(absolutePath, bytes);
  return absolutePath;
}

async function makeStage(parent, platform = 'linux-x86_64') {
  const metadata = HEADLESS_TARGETS[platform];
  const stageDirectory = path.join(parent, metadata.target);
  await mkdir(stageDirectory, { recursive: true });
  const serverBytes = text('synthetic server executable bytes');
  const clientBytes = text('synthetic native client executable bytes');
  const payload = new Map([
    ['domios-server/' + metadata.server, serverBytes],
    ['cli/' + metadata.cli, text('synthetic CLI executable bytes')],
    ['dist/index.html', text('<html><head><link rel="stylesheet" href="/assets/site.css"></head><body><script type="module" src="/assets/main.js"></script></body></html>')],
    ['dist/assets/main.js', text('synthetic frontend javascript')],
    ['dist/assets/site.css', text('synthetic frontend stylesheet')],
    ['skills/agent/SKILL.md', text('synthetic runtime skill')],
    ['LICENSE', text('synthetic license')],
    ['NOTICE', text('synthetic notice')],
  ]);
  if (metadata.conpty) {
    payload.set('conpty/' + metadata.conpty + '/OpenConsole.exe', text('synthetic ConPTY host'));
    payload.set('conpty/' + metadata.conpty + '/conpty.dll', text('synthetic ConPTY library'));
  }
  for (const [relativePath, bytes] of payload) await writePayload(stageDirectory, relativePath, bytes);
  const component = {
    schema: 1,
    server_build: 'fixture-server-build',
    server_executable: 'domios-server/' + metadata.server,
    server_sha256: hash(serverBytes),
    client_sha256: hash(clientBytes),
    api_version: 3,
    bootstrap_protocol: 1,
  };
  const componentBytes = text(JSON.stringify(component, null, 2) + '\n');
  payload.set('server-component.json', componentBytes);
  await writePayload(stageDirectory, 'server-component.json', componentBytes);
  const files = [...payload.entries()]
    .sort(([left], [right]) => Buffer.compare(Buffer.from(left), Buffer.from(right)))
    .map(([relativePath, bytes]) => ({ path: relativePath, sha256: hash(bytes) }));
  const receipt = {
    schema_version: 1,
    platform,
    target: metadata.target,
    tag: identity.tag,
    version: identity.version,
    source_sha: identity.sourceSha,
    files,
  };
  await writePayload(stageDirectory, 'headless-stage.json', text(JSON.stringify(receipt, null, 2) + '\n'));
  return { stageDirectory, metadata, payload, serverBytes, componentBytes };
}

async function rewriteInventory(stageDirectory, platform, metadata) {
  const prior = JSON.parse(await readFile(path.join(stageDirectory, 'headless-stage.json'), 'utf8'));
  const paths = [];
  async function visit(directory, prefix = '') {
    for (const name of await readdir(directory)) {
      const relative = prefix ? prefix + '/' + name : name;
      const absolute = path.join(directory, name);
      const stat = await lstat(absolute);
      if (stat.isDirectory()) {
        await visit(absolute, relative);
      } else if (relative !== 'headless-stage.json') {
        const bytes = await readFile(absolute);
        paths.push({ path: relative, sha256: hash(bytes) });
      }
    }
  }
  await visit(stageDirectory);
  paths.sort((left, right) => Buffer.compare(Buffer.from(left.path), Buffer.from(right.path)));
  const receipt = { ...prior, platform, target: metadata.target, files: paths };
  await writeFile(path.join(stageDirectory, 'headless-stage.json'), JSON.stringify(receipt, null, 2) + '\n');
}

async function withTemp(t, run) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'domios-headless-fixture-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  return run(root);
}

const expectedTargets = {
  'darwin-aarch64': 'aarch64-apple-darwin',
  'darwin-x86_64': 'x86_64-apple-darwin',
  'linux-aarch64': 'aarch64-unknown-linux-gnu',
  'linux-x86_64': 'x86_64-unknown-linux-gnu',
  'windows-aarch64': 'aarch64-pc-windows-msvc',
  'windows-x86_64': 'x86_64-pc-windows-msvc',
};

test('finite target mapping and headless asset names preserve platform identity', () => {
  assert.deepEqual(Object.fromEntries(Object.entries(HEADLESS_TARGETS).map(([platform, value]) => [platform, value.target])), expectedTargets);
  assert.deepEqual(headlessAssetNames(identity.tag, 'linux-x86_64'), {
    archive: 'domios-headless-v1.2.3-linux-x86_64.tar.gz',
    checksum: 'domios-headless-v1.2.3-linux-x86_64.tar.gz.sha256',
    manifest: 'domios-headless-v1.2.3-linux-x86_64.tar.gz.manifest.json',
  });
});

test('synthetic full and headless variants retain identical common server bytes', async t => withTemp(t, async root => {
  const stage = await makeStage(root);
  const validated = await validateHeadlessStage({
    stageDirectory: stage.stageDirectory,
    target: stage.metadata.target,
    ...identity,
  });
  const fullFixtureServer = Buffer.from(stage.serverBytes);
  const outputDirectory = path.join(root, 'release-artifacts');
  const result = await packageHeadless({
    stageDirectory: stage.stageDirectory,
    outputDirectory,
    target: stage.metadata.target,
    ...identity,
  });
  const archive = await readFile(result.archivePath);
  const entries = readHeadlessArchive(archive);
  const serverPath = 'domios-server/' + stage.metadata.server;
  const headlessServer = entries.get(serverPath);
  assert.ok(headlessServer);
  assert.deepEqual(headlessServer.bytes, fullFixtureServer);
  assert.equal(hash(headlessServer.bytes), stage.payload.get(serverPath) && hash(stage.payload.get(serverPath)));
  assert.equal(entries.get('server-component.json').bytes.equals(stage.componentBytes), true);
  assert.equal(entries.size, validated.files.length);
  assert.equal(entries.has('headless-stage.json'), false);
  assert.equal([...entries.keys()].some(file => /(?:^|\/)(?:domios-client|tauri-runtime)(?:\.|\/|$)/i.test(file)), false);
  assert.equal(headlessServer.mode, 0o755);
  assert.equal(result.platform, 'linux-x86_64');
  assert.equal(result.target, stage.metadata.target);
  assert.equal(result.tag, identity.tag);
  assert.equal(result.version, identity.version);
  assert.equal(result.source_sha, identity.sourceSha);
  assert.equal(result.archive_sha256, hash(archive));
  assert.equal(await readFile(result.checksumPath, 'utf8'), result.archive_sha256 + '  ' + path.basename(result.archivePath) + '\n');
  const manifest = JSON.parse(await readFile(result.manifestPath, 'utf8'));
  assert.equal(manifest.variant, 'headless');
  assert.equal(manifest.archive_sha256, result.archive_sha256);
  const repeated = await packageHeadless({
    stageDirectory: stage.stageDirectory,
    outputDirectory,
    target: stage.metadata.target,
    ...identity,
  });
  assert.deepEqual(await readFile(repeated.archivePath), archive);
}));

test('Windows arm64 package requires and carries only the target ConPTY pair', async t => withTemp(t, async root => {
  const stage = await makeStage(root, 'windows-aarch64');
  const validated = await validateHeadlessStage({
    stageDirectory: stage.stageDirectory,
    target: stage.metadata.target,
    ...identity,
  });
  const paths = validated.files.map(file => file.path);
  assert.ok(paths.includes('domios-server/domios-server.exe'));
  assert.ok(paths.includes('cli/codeterm.exe'));
  assert.ok(paths.includes('conpty/arm64/OpenConsole.exe'));
  assert.ok(paths.includes('conpty/arm64/conpty.dll'));
  assert.equal(paths.some(file => file.startsWith('conpty/x64/')), false);
  const result = await packageHeadless({
    stageDirectory: stage.stageDirectory,
    outputDirectory: path.join(root, 'release-artifacts'),
    target: stage.metadata.target,
    ...identity,
  });
  const entries = readHeadlessArchive(await readFile(result.archivePath));
  assert.ok(entries.has('domios-server/domios-server.exe'));
  assert.ok(entries.has('conpty/arm64/OpenConsole.exe'));
  assert.ok(entries.has('conpty/arm64/conpty.dll'));
}));

test('missing resources, extra files, identity mismatch, and component digest drift refuse packaging', async t => withTemp(t, async root => {
  const missingRoot = path.join(root, 'missing');
  const missing = await makeStage(missingRoot);
  await unlink(path.join(missing.stageDirectory, 'skills', 'agent', 'SKILL.md'));
  await rewriteInventory(missing.stageDirectory, 'linux-x86_64', missing.metadata);
  await assert.rejects(validateHeadlessStage({
    stageDirectory: missing.stageDirectory,
    target: missing.metadata.target,
    ...identity,
  }), /required skills resources are missing/);

  const frontendRoot = path.join(root, 'frontend');
  const frontend = await makeStage(frontendRoot);
  await unlink(path.join(frontend.stageDirectory, 'dist', 'assets', 'main.js'));
  await rewriteInventory(frontend.stageDirectory, 'linux-x86_64', frontend.metadata);
  await assert.rejects(validateHeadlessStage({
    stageDirectory: frontend.stageDirectory,
    target: frontend.metadata.target,
    ...identity,
  }), /frontend asset referenced by dist\/index.html is missing/);

  const extraRoot = path.join(root, 'extra');
  const extra = await makeStage(extraRoot);
  await writePayload(extra.stageDirectory, 'config/user.json', text('{}'));
  await assert.rejects(validateHeadlessStage({
    stageDirectory: extra.stageDirectory,
    target: extra.metadata.target,
    ...identity,
  }), /extra directory/);

  const identityRoot = path.join(root, 'identity');
  const mismatch = await makeStage(identityRoot);
  await assert.rejects(validateHeadlessStage({
    stageDirectory: mismatch.stageDirectory,
    target: mismatch.metadata.target,
    ...identity,
    sourceSha: 'b'.repeat(40),
  }), /source SHA/);

  const componentRoot = path.join(root, 'component');
  const drift = await makeStage(componentRoot);
  const componentPath = path.join(drift.stageDirectory, 'server-component.json');
  const component = JSON.parse(await readFile(componentPath, 'utf8'));
  component.server_sha256 = '0'.repeat(64);
  await writeFile(componentPath, JSON.stringify(component, null, 2) + '\n');
  await rewriteInventory(drift.stageDirectory, 'linux-x86_64', drift.metadata);
  await assert.rejects(validateHeadlessStage({
    stageDirectory: drift.stageDirectory,
    target: drift.metadata.target,
    ...identity,
  }), /does not match the staged server bytes/);

  const conptyRoot = path.join(root, 'conpty');
  const conpty = await makeStage(conptyRoot, 'windows-x86_64');
  await unlink(path.join(conpty.stageDirectory, 'conpty', 'x64', 'conpty.dll'));
  await rewriteInventory(conpty.stageDirectory, 'windows-x86_64', conpty.metadata);
  await assert.rejects(validateHeadlessStage({
    stageDirectory: conpty.stageDirectory,
    target: conpty.metadata.target,
    ...identity,
  }), /required headless payload is missing: conpty\/x64\/conpty.dll/);
}));
