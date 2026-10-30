#!/usr/bin/env node
import { createHash, randomUUID } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { chmod, copyFile, lstat, mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertServerClosure, serverArtifact, SERVER_FEATURES } from './server-artifact.mjs';
import { SERVER_COMPONENT_SCHEMA, SUPPORTED_BOOTSTRAP_PROTOCOL, validateManifest } from './package-server-component-manifest.mjs';
import { HEADLESS_TARGETS, packageHeadless, validateHeadlessStage } from './headless-release-package.mjs';
import { prepareCompileBundleStubs } from './prepare-bundle-resource-stubs.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const targetMetadata = new Map(Object.entries(HEADLESS_TARGETS).map(([platform, metadata]) => [metadata.target, { platform, ...metadata }]));
const SHA_PATTERN = /^[a-f0-9]{40}$/;
const HASH_PATTERN = /^[a-f0-9]{64}$/;

function fail(message) {
  throw new Error(message);
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function metadataFor(target) {
  const metadata = targetMetadata.get(target);
  if (!metadata) fail('unsupported release target: ' + target);
  return metadata;
}

async function regularBytes(file, label) {
  const stat = await lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size === 0) fail(label + ' must be a non-empty regular file: ' + file);
  return readFile(file);
}

function bundledCliPath(repo, metadata) {
  const name = metadata.platform.startsWith('windows-') ? `codeterm-${metadata.target}.exe` : 'codeterm';
  return path.join(repo, 'src-tauri', 'bundled-cli', name);
}

export async function releaseCliDigest({ repo = root, target }) {
  const metadata = metadataFor(target);
  return sha256(await regularBytes(bundledCliPath(repo, metadata), 'signed bundled CLI'));
}

function run(program, args, cwd) {
  return execFileSync(program, args, { cwd, encoding: 'utf8', timeout: 120000, maxBuffer: 8 * 1024 * 1024 });
}

function macSigningTeam(binary) {
  const result = spawnSync('codesign', ['-dv', '--verbose=4', binary], { encoding: 'utf8' });
  if (result.status !== 0) fail('approved macOS code signature could not be inspected');
  const details = `${result.stdout}\n${result.stderr}`;
  const team = /^TeamIdentifier=([A-Z0-9]{10})$/m.exec(details)?.[1];
  if (!team || !details.includes('Authority=Developer ID Application') ||
      !process.env.APPLE_TEAM_ID || team !== process.env.APPLE_TEAM_ID) {
    fail('macOS code signature differs from the approved Developer ID team');
  }
  return team;
}

export function fullComponentManifest({ target, sourceSha, serverSha256, clientSha256, teamId }) {
  const metadata = metadataFor(target);
  const mac = metadata.platform.startsWith('darwin-');
  const clientIdentity = mac
    ? { kind: 'macos_designated_requirement',
        requirement: `anchor apple generic and identifier "com.domios.app" and certificate leaf[subject.OU] = "${teamId}"`,
        team_id: teamId, identifier: 'com.domios.app' }
    : { kind: 'sha256', sha256: clientSha256 };
  return validateManifest({ schema: SERVER_COMPONENT_SCHEMA, server_build: sourceSha,
    server_executable: mac ? '../MacOS/domios-server' : `domios-server/${metadata.server}`,
    server_sha256: serverSha256, client_identity: clientIdentity,
    api_version: 3, bootstrap_protocol: SUPPORTED_BOOTSTRAP_PROTOCOL });
}

export function headlessComponentManifest(full, target) {
  const metadata = metadataFor(target);
  const reviewed = validateManifest(full);
  return validateManifest({ ...reviewed, server_executable: `domios-server/${metadata.server}`,
    client_identity: { kind: 'none' } });
}

export function verifyFinalPair({ target, sourceSha, manifest, stagedServer, packagedServer,
  stagedCli, packagedCli, packagedClient, stagedManifest, packagedManifest, paths = {} }) {
  const metadata = metadataFor(target);
  const reviewed = validateManifest(manifest);
  if (!Buffer.from(stagedManifest).equals(Buffer.from(packagedManifest))) fail('full packaged manifest differs from finalized stage');
  if (reviewed.server_build !== sourceSha || reviewed.server_sha256 !== sha256(stagedServer) ||
      !Buffer.from(stagedServer).equals(Buffer.from(packagedServer))) {
    fail(`full packaged server differs from common final server: ${paths.server ?? 'final server'} != ${paths.packagedServer ?? 'packaged server'}`);
  }
  if (!Buffer.from(stagedCli).equals(Buffer.from(packagedCli))) {
    fail(`full packaged CLI differs from common final CLI: ${paths.cli ?? 'final CLI'} != ${paths.packagedCli ?? 'packaged CLI'}`);
  }
  if (metadata.platform.startsWith('darwin-')) {
    if (reviewed.client_identity.kind !== 'macos_designated_requirement') fail('Mac full app lacks its designated requirement');
  } else if (reviewed.client_identity.kind !== 'sha256' ||
             reviewed.client_identity.sha256 !== sha256(packagedClient)) {
    fail('full packaged client differs from the component identity');
  }
  return reviewed;
}

async function cargoMessages(file) {
  return (await readFile(file, 'utf8')).split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
}

export function verifyServerSource(messages, source) {
  const artifacts = messages.filter(row => row.reason === 'compiler-artifact' && row.target?.name === 'domios-server' &&
    row.target.kind?.includes('bin') && row.executable && !row.profile?.test);
  if (artifacts.length !== 1) fail(`expected one release server binary; found ${artifacts.length}`);
  const actual = realpathSync.native(artifacts[0].target.src_path);
  const expected = realpathSync.native(source);
  if (actual !== expected) fail(`release server source differs from owned source: ${actual} != ${expected}`);
  return artifacts[0];
}

export function assertReleaseServerFeatures(features) {
  const expected = SERVER_FEATURES.map(feature => feature.split('/')[1]).sort();
  if (JSON.stringify([...features].sort()) !== JSON.stringify(expected)) {
    fail('release server feature selection differs from the reviewed closure');
  }
}

export function checkReleaseServerFeatures(buildScript) {
  const inputs = buildScript.replaceAll('\r\n', '\n')
    .match(/^prepare_release_bundle_inputs\(\) \{\n[\s\S]*?^\}/m)?.[0];
  if (!inputs) fail('release server build function is missing');
  const commands = inputs.replace(/\\\n\s*/g, ' ');
  for (const operation of ['build', 'tree']) {
    const rows = commands.split('\n').filter(line =>
      new RegExp(`^\\s*cargo ${operation} .* -p domios-server(?: |$)`).test(line));
    if (rows.length !== 1) fail(`expected one release server cargo ${operation} command`);
    const features = rows[0].match(/--features ([^\s]+)/)?.[1].split(',');
    if (!rows[0].includes('--no-default-features') || !features ||
        features.some(feature => !feature.startsWith('domios-server/'))) {
      fail(`release server cargo ${operation} requires explicit isolated features`);
    }
    assertReleaseServerFeatures(features.map(feature => feature.slice('domios-server/'.length)));
  }
}

export async function verifyServerBuild({ repo = root, target, sourceSha, cargoLog, treeLog, expectedCliSha256 }) {
  metadataFor(target);
  if (!SHA_PATTERN.test(sourceSha ?? '')) fail('source SHA must be a full commit identity');
  if (!HASH_PATTERN.test(expectedCliSha256 ?? '')) fail('expected CLI SHA-256 is missing');
  if (run('git', ['rev-parse', 'HEAD'], repo).trim() !== sourceSha) fail('server build source differs from checkout HEAD');
  if (await releaseCliDigest({ repo, target }) !== expectedCliSha256) fail('server build CLI binding differs from final bundled CLI');
  const messages = await cargoMessages(cargoLog);
  if (!messages.some(row => row.reason === 'build-finished' && row.success)) fail('release server Cargo build did not finish');
  const source = path.join(repo, 'src-tauri', 'crates', 'domios-server', 'src', 'main.rs');
  const artifact = verifyServerSource(messages, source);
  assertReleaseServerFeatures(artifact.features);
  const binary = serverArtifact(messages);
  const expectedName = target.startsWith('aarch64-pc-windows') || target.startsWith('x86_64-pc-windows')
    ? 'domios-server.exe' : 'domios-server';
  if (path.basename(binary) !== expectedName) fail('server executable has the wrong target name');
  const buildReceipts = messages.filter(row => row.reason === 'build-script-executed' && row.package_id?.includes('domios-server'));
  if (buildReceipts.length !== 1) fail('expected one release server build-script receipt');
  const environment = Object.fromEntries(buildReceipts[0].env ?? []);
  if (environment.BUILD_SHA !== sourceSha || environment.SERVER_CLI_SHA256 !== expectedCliSha256) {
    fail('server build receipt does not bind the exact source and bundled CLI');
  }
  const packages = (await readFile(treeLog, 'utf8')).split(/\r?\n/).map(line => line.trim()).filter(Boolean);
  assertServerClosure(packages);
  await regularBytes(binary, 'release server executable');
  return binary;
}

export async function signAndVerifyServer(options) {
  const binary = await verifyServerBuild(options);
  if (options.target.endsWith('-apple-darwin')) {
    const identity = process.env.APPLE_SIGNING_IDENTITY;
    const keychain = process.env.RELEASE_KEYCHAIN;
    if (!identity || !keychain) fail('Apple signing identity and release keychain are required for the final server');
    run('codesign', ['--force', '--options', 'runtime', '--timestamp', '--keychain', keychain, '--sign', identity, binary], options.repo ?? root);
    run('codesign', ['--verify', '--strict', binary], options.repo ?? root);
  }
  return { binary, sha256: sha256(await regularBytes(binary, 'final release server')) };
}

export async function prepareNativeBuildResources({ repo = root, target }) {
  metadataFor(target);
  await regularBytes(path.join(repo, 'dist', 'index.html'), 'built frontend entry');
  const final = path.join(repo, 'target', 'domios-package');
  const temporary = final + '.' + randomUUID() + '.tmp';
  try {
    await copyRegularTree(path.join(repo, 'dist'), path.join(temporary, 'dist'));
    await replaceStage(temporary, final);
    prepareCompileBundleStubs({ root: repo, target });
    return { stageDirectory: final, frontendFileCount: (await inventory(path.join(final, 'dist'))).length };
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

export async function verifyPreparedFrontend({ repo = root }) {
  const source = await inventory(path.join(repo, 'dist'));
  const prepared = await inventory(path.join(repo, 'target', 'domios-package', 'dist'));
  if (source.length === 0 || JSON.stringify(source) !== JSON.stringify(prepared)) {
    fail('native build frontend differs from final package frontend');
  }
}

export async function stageFullResources({ repo = root, target, sourceSha, cargoLog, treeLog, clientFile }) {
  await verifyPreparedFrontend({ repo });
  const metadata = metadataFor(target);
  const cliSha256 = await releaseCliDigest({ repo, target });
  const server = await verifyServerBuild({ repo, target, sourceSha, cargoLog, treeLog, expectedCliSha256: cliSha256 });
  const client = path.resolve(clientFile);
  const clientSha256 = sha256(await regularBytes(client, 'final native client'));
  const mac = metadata.platform.startsWith('darwin-');
  if (path.basename(client) !== (metadata.platform.startsWith('windows-') ? 'domios-app.exe' : 'domios-app')) {
    fail('final native client has an unexpected executable name');
  }
  const teamId = mac ? macSigningTeam(server) : null;
  const manifest = fullComponentManifest({ target, sourceSha,
    serverSha256: sha256(await regularBytes(server, 'final server')), clientSha256, teamId });
  const final = path.join(repo, 'target', 'domios-package');
  const temporary = final + '.' + randomUUID() + '.tmp';
  try {
    await copyRegularTree(server, path.join(temporary, 'domios-server', metadata.server));
    await copyRegularTree(bundledCliPath(repo, metadata), path.join(temporary, 'cli', metadata.cli));
    await copyRegularTree(bundledCliPath(repo, metadata), path.join(temporary, 'cli', metadata.cli.replace('codeterm', 'domios')));
    for (const name of ['dist', 'skills', 'LICENSE', 'NOTICE']) {
      await copyRegularTree(path.join(repo, name), path.join(temporary, name));
    }
    if (metadata.conpty) {
      for (const name of ['OpenConsole.exe', 'conpty.dll']) {
        await copyRegularTree(path.join(repo, 'src-tauri', 'conpty', metadata.conpty, name),
          path.join(temporary, 'conpty', metadata.conpty, name));
      }
    }
    if (mac) await copyRegularTree(server, path.join(temporary, 'domios-server', `domios-server-${target}`));
    await writeFile(path.join(temporary, 'server-component.json'), JSON.stringify(manifest, null, 2) + '\n', { flag: 'wx' });
    await replaceStage(temporary, final);
    return { stageDirectory: final, serverSha256: manifest.server_sha256, clientSha256, cliSha256,
      clientIdentity: manifest.client_identity, alias: mac ? path.join(final, 'domios-server', `domios-server-${target}`) : null };
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

async function copyRegularTree(source, destination) {
  const stat = await lstat(source);
  if (stat.isSymbolicLink()) fail('runtime payload includes a link: ' + source);
  if (stat.isFile()) {
    await mkdir(path.dirname(destination), { recursive: true });
    await copyFile(source, destination);
    if (process.platform !== 'win32') await chmod(destination, stat.mode & 0o777);
    return;
  }
  if (!stat.isDirectory()) fail('runtime payload includes a non-regular entry: ' + source);
  await mkdir(destination, { recursive: true });
  for (const name of await readdir(source)) await copyRegularTree(path.join(source, name), path.join(destination, name));
}

function compareUtf8(left, right) {
  return Buffer.compare(Buffer.from(left, 'utf8'), Buffer.from(right, 'utf8'));
}

async function inventory(stage) {
  const rows = [];
  async function visit(directory, prefix = '') {
    for (const name of (await readdir(directory)).sort(compareUtf8)) {
      const relative = prefix ? prefix + '/' + name : name;
      const absolute = path.join(directory, name);
      const stat = await lstat(absolute);
      if (stat.isSymbolicLink()) fail('common stage includes a link: ' + relative);
      if (stat.isDirectory()) await visit(absolute, relative);
      else if (stat.isFile() && relative !== 'headless-stage.json') rows.push({ path: relative, sha256: sha256(await readFile(absolute)) });
      else fail('common stage includes a non-regular entry: ' + relative);
    }
  }
  await visit(stage);
  rows.sort((left, right) => compareUtf8(left.path, right.path));
  return rows;
}

async function extractedFiles(directory) {
  const files = [];
  async function visit(current) {
    for (const name of await readdir(current)) {
      const file = path.join(current, name);
      const stat = await lstat(file);
      if (stat.isDirectory()) await visit(file);
      else if (stat.isFile()) files.push(file);
    }
  }
  await visit(directory);
  return files;
}

function one(files, predicate, label) {
  const matches = files.filter(predicate);
  if (matches.length !== 1) fail(`expected one ${label} in the full package; found ${matches.length}`);
  return matches[0];
}

export async function extractFullPair({ bundle, platform, target, scratch }) {
  const metadata = metadataFor(target);
  if (metadata.platform !== platform) fail('full package platform differs from stage target');
  if (platform.startsWith('darwin-')) run('tar', ['-xzf', bundle, '-C', scratch], root);
  else if (platform.startsWith('linux-')) run('dpkg-deb', ['--extract', bundle, scratch], root);
  else {
    const standard7z = 'C:\\Program Files\\7-Zip\\7z.exe';
    const sevenZip = process.env.RELEASE_7ZIP || (await lstat(standard7z).then(() => standard7z, () => '7z'));
    run(sevenZip, ['x', '-y', `-o${scratch}`, bundle], root);
  }
  const files = await extractedFiles(scratch);
  const inCli = file => /[/\\]cli[/\\]/.test(file);
  const client = one(files, file => path.basename(file).toLowerCase() === (platform.startsWith('windows-') ? 'domios-app.exe' : 'domios-app') &&
    !inCli(file) && (!platform.startsWith('darwin-') || file.includes(`${path.sep}MacOS${path.sep}`)), 'packaged native client');
  const cli = one(files, file => inCli(file) && (path.basename(file) === metadata.cli ||
    path.basename(file) === `codeterm-${target}.exe`), 'packaged CLI');
  const server = one(files, file => path.basename(file) === metadata.server &&
    (platform.startsWith('darwin-') ? file.includes(`${path.sep}Contents${path.sep}MacOS${path.sep}`)
      : file.includes(`${path.sep}domios-server${path.sep}`)), 'packaged server');
  const component = one(files, file => path.basename(file) === 'server-component.json', 'packaged server component manifest');
  return { client, cli, server, component, files };
}

async function replaceStage(temporary, final) {
  const backup = final + '.' + randomUUID() + '.bak';
  let hadPrior = false;
  try {
    await rename(final, backup);
    hadPrior = true;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  try {
    await rename(temporary, final);
  } catch (error) {
    if (hadPrior) await rename(backup, final);
    throw error;
  }
  if (hadPrior) await rm(backup, { recursive: true, force: true });
}

export async function produceCommonStage({ repo = root, target, tag, version, sourceSha, cargoLog, treeLog, bundle, outputDirectory }) {
  metadataFor(target);
  if (tag !== `v${version}` || !SHA_PATTERN.test(sourceSha ?? '')) fail('stage tag/version/source identity is invalid');
  if (run('git', ['rev-parse', 'HEAD'], repo).trim() !== sourceSha) fail('stage source SHA differs from checkout HEAD');
  const cliHash = await releaseCliDigest({ repo, target });
  const server = await verifyServerBuild({ repo, target, sourceSha, cargoLog, treeLog, expectedCliSha256: cliHash });
  return produceVerifiedCommonStage({ repo, target, tag, version, sourceSha, bundle, outputDirectory, server, cliHash });
}

export async function produceVerifiedCommonStage({ repo = root, target, tag, version, sourceSha, bundle, outputDirectory, server, cliHash }) {
  const metadata = metadataFor(target);
  if (tag !== `v${version}` || !SHA_PATTERN.test(sourceSha ?? '') || !HASH_PATTERN.test(cliHash ?? '')) fail('invalid verified stage identity');
  if (run('git', ['rev-parse', 'HEAD'], repo).trim() !== sourceSha) fail('verified stage source SHA differs from checkout HEAD');
  const outputRoot = path.resolve(outputDirectory ?? path.join(repo, 'release-artifacts'));
  const commonRoot = path.join(repo, 'release-artifacts', 'common-stage');
  const final = path.join(commonRoot, target);
  const temporaryParent = path.join(commonRoot, '.' + randomUUID() + '.tmp');
  const temporary = path.join(temporaryParent, target);
  const scratch = await mkdtemp(path.join(os.tmpdir(), 'domios-release-full-'));
  await mkdir(commonRoot, { recursive: true });
  try {
    const { client, cli, server: packagedServer, component } =
      await extractFullPair({ bundle, platform: metadata.platform, target, scratch });
    const fullStage = path.join(repo, 'target', 'domios-package');
    const fullManifestBytes = await regularBytes(path.join(fullStage, 'server-component.json'), 'staged full manifest');
    const fullManifest = verifyFinalPair({ target, sourceSha, manifest: JSON.parse(fullManifestBytes.toString('utf8')),
      stagedServer: await regularBytes(server, 'final server'),
      packagedServer: await regularBytes(packagedServer, 'full packaged server'),
      stagedCli: await regularBytes(path.join(fullStage, 'cli', metadata.cli), 'staged full CLI'),
      packagedCli: await regularBytes(cli, 'full packaged CLI'),
      packagedClient: await regularBytes(client, 'full packaged client'),
      stagedManifest: fullManifestBytes, packagedManifest: await regularBytes(component, 'full packaged manifest'),
      paths: { server, packagedServer, cli: path.join(fullStage, 'cli', metadata.cli), packagedCli: cli } });
    if (sha256(await regularBytes(cli, 'full packaged CLI')) !== cliHash) fail('full packaged CLI differs from signed CLI');
    const alias = path.join(path.dirname(cli), metadata.cli.replace('codeterm', 'domios'));
    if (sha256(await regularBytes(alias, 'full packaged CLI alias')) !== cliHash) fail('full packaged CLI alias differs from signed CLI');
    if (metadata.platform.startsWith('linux-')) {
      for (const name of ['domios', 'codeterm']) {
        if (sha256(await regularBytes(path.join(scratch, 'usr', 'bin', name), 'system CLI')) !== cliHash) {
          fail('system CLI differs from signed CLI: ' + name);
        }
      }
    }
    if (metadata.platform.startsWith('darwin-')) {
      const app = path.dirname(path.dirname(path.dirname(client)));
      run('codesign', ['--verify', '--deep', '--strict', app], repo);
      if (macSigningTeam(app) !== fullManifest.client_identity.team_id) fail('full app code identity differs from its sealed manifest');
    }
    await copyRegularTree(path.join(fullStage, 'domios-server', metadata.server), path.join(temporary, 'domios-server', metadata.server));
    await copyRegularTree(path.join(fullStage, 'cli', metadata.cli), path.join(temporary, 'cli', metadata.cli));
    await copyRegularTree(path.join(fullStage, 'cli', metadata.cli.replace('codeterm', 'domios')), path.join(temporary, 'cli', metadata.cli.replace('codeterm', 'domios')));
    for (const name of ['dist', 'skills', 'LICENSE', 'NOTICE']) {
      await copyRegularTree(path.join(fullStage, name), path.join(temporary, name));
    }
    if (metadata.conpty) {
      for (const name of ['OpenConsole.exe', 'conpty.dll']) {
        await copyRegularTree(path.join(fullStage, 'conpty', metadata.conpty, name),
          path.join(temporary, 'conpty', metadata.conpty, name));
      }
    }
    const headlessManifest = headlessComponentManifest(fullManifest, target);
    await writeFile(path.join(temporary, 'server-component.json'), JSON.stringify(headlessManifest, null, 2) + '\n', { flag: 'wx' });
    const receipt = { schema_version: 1, platform: metadata.platform, target, tag, version, source_sha: sourceSha,
      files: await inventory(temporary) };
    await writeFile(path.join(temporary, 'headless-stage.json'), JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
    await validateHeadlessStage({ stageDirectory: temporary, target, tag, version, sourceSha });
    await replaceStage(temporary, final);
    const headless = await packageHeadless({ stageDirectory: final, outputDirectory: outputRoot, target, tag, version, sourceSha });
    return { stageDirectory: final, clientSha256: sha256(await regularBytes(client, 'full packaged client')),
      serverSha256: sha256(await regularBytes(server, 'final server')), cliSha256: cliHash, ...headless };
  } finally {
    await rm(scratch, { recursive: true, force: true });
    await rm(temporaryParent, { recursive: true, force: true });
  }
}

function argumentsFor(args) {
  const values = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    const value = args[index + 1];
    if (!name?.startsWith('--') || value === undefined || value.startsWith('--') || values.has(name)) fail('invalid release-stage argument');
    values.set(name, value);
  }
  return name => {
    if (!values.has(name)) fail('missing release-stage argument ' + name);
    return values.get(name);
  };
}

async function main() {
  const command = process.argv[2];
  if (command === 'check-server-features') {
    checkReleaseServerFeatures(await readFile(path.join(root, 'build.sh'), 'utf8'));
    process.stdout.write('release server features match build and dependency tree\n');
    return;
  }
  const arg = argumentsFor(process.argv.slice(3));
  const target = arg('--target');
  if (command === 'cli-digest') {
    process.stdout.write(await releaseCliDigest({ target }) + '\n');
  } else if (command === 'stage-build-resources') {
    process.stdout.write(JSON.stringify(await prepareNativeBuildResources({ target })) + '\n');
  } else if (command === 'prepare') {
    const result = await signAndVerifyServer({ target, sourceSha: arg('--source-sha'), cargoLog: arg('--cargo-log'),
      treeLog: arg('--tree-log'), expectedCliSha256: arg('--cli-sha256') });
    process.stdout.write(JSON.stringify(result) + '\n');
  } else if (command === 'stage-full') {
    const result = await stageFullResources({ target, sourceSha: arg('--source-sha'), cargoLog: arg('--cargo-log'),
      treeLog: arg('--tree-log'), clientFile: arg('--client-file') });
    process.stdout.write(JSON.stringify(result) + '\n');
  } else if (command === 'finalize') {
    const result = await produceCommonStage({ target, tag: arg('--tag'), version: arg('--version'),
      sourceSha: arg('--source-sha'), cargoLog: arg('--cargo-log'), treeLog: arg('--tree-log'),
      bundle: arg('--bundle'), outputDirectory: arg('--output-dir') });
    process.stdout.write(JSON.stringify(result) + '\n');
  } else fail('unknown release-stage command');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    process.stderr.write('release stage refused: ' + error.message + '\n');
    process.exitCode = 1;
  });
}
