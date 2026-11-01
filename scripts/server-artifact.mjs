// Builds the isolated server oracle from Cargo artifact messages; no test feature union is accepted.
import { createHash, randomUUID } from 'node:crypto';
import { readFileSync, realpathSync, writeFileSync, mkdirSync, copyFileSync, cpSync, chmodSync, lstatSync, readdirSync, renameSync, rmSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { stagedDevPaths } from './dev-native-layout.mjs';
import { syncBuildDirectory } from './build-artifacts.cjs';
import { macDevSigning, signMacDevServer, sealMacDevBundle } from './dev-macos-bundle.mjs';
import { createRequire } from 'node:module';
const { bundledCliName } = createRequire(import.meta.url)('./bundle-cli.js');

export const SERVER_FEATURES = ['domios-server/embeddings', 'domios-server/mem-postgres'];
export function assertServerClosure(packages) {
  if (!packages.some(row => row.startsWith('domios-server '))) throw new Error('Server dependency root missing');
  const forbidden = packages.filter(row => /^(?:tauri(?:-| )|wry(?: |$)|webview(?:2)?(?:-| )|web-view(?:-| )|webkit2gtk(?:-| )|codeterm-client-shell(?: |$))/.test(row));
  if (forbidden.length) throw new Error(`Server production closure includes forbidden packages: ${forbidden.map(row => row.split(' ')[0]).join(', ')}`);
}
export function serverArtifact(messages) {
  const artifacts = messages.filter(message => message.reason === 'compiler-artifact' && message.target?.name === 'domios-server' && message.target.kind.includes('bin') && message.executable && !message.profile?.test);
  if (artifacts.length !== 1) throw new Error(`Expected one server binary artifact; received ${artifacts.length}`);
  return realpathSync.native(artifacts[0].executable);
}
export function artifactReceipt({ cargoLog, treeLog, build, target, resources, platform = process.platform }) {
  const messages = readFileSync(cargoLog, 'utf8').split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
  if (!messages.some(message => message.reason === 'build-finished' && message.success)) throw new Error('Cargo build did not finish successfully');
  const dependency_packages = readFileSync(treeLog, 'utf8').split(/\r?\n/).map(line => line.trim()).filter(Boolean);
  assertServerClosure(dependency_packages);
  const executable = serverArtifact(messages);
  const artifact = messages.find(message => message.reason === 'compiler-artifact' && message.executable && realpathSync.native(message.executable) === executable);
  if (JSON.stringify([...artifact.features].sort()) !== JSON.stringify(['embeddings', 'mem-postgres'])) throw new Error('Unexpected server artifact feature selection');
  if (path.basename(executable) !== `domios-server${platform === 'win32' ? '.exe' : ''}`) throw new Error('Unexpected server executable filename');
  return { executable, executable_sha256: createHash('sha256').update(readFileSync(executable)).digest('hex'), build, target, profile: 'dev', features: SERVER_FEATURES, resources: realpathSync.native(resources), dependency_packages };
}
export function workerArtifact(messages, repo) {
  const source = realpathSync.native(path.join(repo, 'cli/src/main.rs'));
  const artifacts = messages.filter(message => message.reason === 'compiler-artifact' && message.target?.name === 'codeterm' && message.target.kind.includes('bin') && message.executable && !message.profile?.test && realpathSync.native(message.target.src_path) === source);
  if (artifacts.length !== 1) throw new Error('Expected the exact CLI package binary artifact');
  return realpathSync.native(artifacts[0].executable);
}
export function stageResources(cliLog, repo, output) {
  const messages = readFileSync(cliLog, 'utf8').split(/\r?\n/).filter(Boolean).map(JSON.parse);
  if (!messages.some(message => message.reason === 'build-finished' && message.success)) throw new Error('CLI build did not finish');
  const executable = workerArtifact(messages, repo);
  const name = `codeterm${process.platform === 'win32' ? '.exe' : ''}`;
  mkdirSync(path.join(output, 'cli'), { recursive: true });
  const target = path.join(output, 'cli', name);
  copyFileSync(executable, target); chmodSync(target, 0o755);
  if (process.platform === 'win32') cpSync(path.join(repo, 'src-tauri/conpty'), path.join(output, 'conpty'), { recursive: true });
  return createHash('sha256').update(readFileSync(target)).digest('hex');
}
function regular(file) {
  const stat = lstatSync(file);
  if (!stat.isFile() || stat.size === 0) throw new Error(`Expected a non-empty regular artifact: ${file}`);
  return realpathSync.native(file);
}
function ensureDirectory(directory) {
  try {
    const stat = lstatSync(directory);
    if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error(`Resource directory is not owned regular storage: ${directory}`);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    ensureDirectory(path.dirname(directory));
    mkdirSync(directory);
  }
}
function copyResource(source, destination) {
  const stat = lstatSync(source);
  if (stat.isSymbolicLink()) throw new Error(`Resource links are not accepted: ${source}`);
  try {
    if (lstatSync(destination).isSymbolicLink()) throw new Error(`Resource destination is a link: ${destination}`);
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (stat.isDirectory()) {
    ensureDirectory(destination);
    for (const name of readdirSync(source).sort()) copyResource(path.join(source, name), path.join(destination, name));
  } else if (stat.isFile()) {
    ensureDirectory(path.dirname(destination));
    try {
      if (readFileSync(source).equals(readFileSync(destination))) return;
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    copyFileSync(source, destination);
  } else throw new Error(`Resource is not a regular file or directory: ${source}`);
}

function pruneRetiredExecutables(destination, justRetired, remove) {
  const directory = path.dirname(destination);
  const prefix = `${path.basename(destination)}.`;
  try {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (!entry.isFile() || !entry.name.startsWith(prefix) || !entry.name.endsWith('.retired')) continue;
      const file = path.join(directory, entry.name);
      if (file === justRetired) continue;
      try { remove(file); } catch { /* Windows retains images held by a running process. */ }
    }
  } catch { /* Retention must not fail a successful publication. */ }
}

export function publishDevExecutable(source, destination, { platform = process.platform, rename = renameSync, remove = rmSync } = {}) {
  const bytes = readFileSync(regular(source));
  let existing = false;
  try {
    regular(destination);
    existing = true;
    if (readFileSync(destination).equals(bytes)) {
      if (platform === 'win32') pruneRetiredExecutables(destination, undefined, remove);
      return;
    }
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  ensureDirectory(path.dirname(destination));
  const candidate = `${destination}.${randomUUID()}.pending`;
  const retired = `${destination}.${randomUUID()}.retired`;
  let rotated = false;
  try {
    copyFileSync(source, candidate);
    // Windows read pins allow renames while refusing executable byte replacement.
    if (platform === 'win32' && existing) {
      rename(destination, retired);
      rotated = true;
    }
    try { rename(candidate, destination); }
    catch (error) {
      if (rotated) {
        try { rename(retired, destination); }
        catch (rollback) { throw new AggregateError([error, rollback], 'DEV server publication and rollback failed'); }
      }
      throw error;
    }
    if (platform === 'win32') pruneRetiredExecutables(destination, rotated ? retired : undefined, remove);
  } finally { rmSync(candidate, { force: true }); }
}

export function resolveDevResourceRoot(resourceRoot, expectedRoot, {
  paths = path, stat = lstatSync, canonical = realpathSync.native, prepare = ensureDirectory,
} = {}) {
  function resolveDirectory(directory) {
    const parent = paths.dirname(directory);
    const resolvedParent = parent === directory ? undefined : resolveDirectory(parent);
    try {
      const entry = stat(directory);
      if (!entry.isDirectory() || entry.isSymbolicLink()) throw new Error('DEV resource directory must not be a symlink or junction');
      return canonical(directory);
    } catch (error) {
      if (error.code !== 'ENOENT' || resolvedParent === undefined) throw error;
      return paths.join(resolvedParent, paths.basename(directory));
    }
  }
  if (resolveDirectory(paths.resolve(resourceRoot)) !== expectedRoot) {
    throw new Error('DEV resource root must derive from the native client artifact directory');
  }
  prepare(expectedRoot);
  if (canonical(expectedRoot) !== expectedRoot) throw new Error('DEV resource root identity changed during staging');
  return expectedRoot;
}

export async function stageDevResources({ cargoLog, treeLog, cliLog, repo, resourceRoot, client, build, serverOnly = false, checkOnly = false, platform = process.platform, env = process.env, signingRun }) {
  if (!/^[a-f0-9]{40}$/.test(build)) throw new Error('Expected exact DEV source build identity');
  const builtClient = regular(client);
  const { mac, resources, client: clientFile, serverRelative } = stagedDevPaths(builtClient, { platform, instance: env.CODETERM_DEV_INSTANCE });
  const root = resolveDevResourceRoot(resourceRoot, resources);
  const server = artifactReceipt({ cargoLog, treeLog, build, resources: root, platform });
  if (path.dirname(server.executable) !== path.dirname(builtClient)) throw new Error('DEV server and client must share the exact Cargo artifact directory');
  if (path.basename(builtClient) !== `domios-app${platform === 'win32' ? '.exe' : ''}`) throw new Error('Unexpected DEV client artifact filename');
  const messages = readFileSync(cargoLog, 'utf8').split(/\r?\n/).filter(Boolean).map(JSON.parse);
  const message = messages.find(row => row.reason === 'compiler-artifact' && row.executable && realpathSync.native(row.executable) === server.executable);
  if (realpathSync.native(message.target.src_path) !== realpathSync.native(path.join(repo, 'src-tauri/crates/domios-server/src/main.rs'))) throw new Error('Server artifact has a foreign source target');
  const cliMessages = readFileSync(cliLog, 'utf8').split(/\r?\n/).filter(Boolean).map(JSON.parse);
  if (!cliMessages.some(row => row.reason === 'build-finished' && row.success)) throw new Error('CLI build did not finish');
  const cli = regular(workerArtifact(cliMessages, repo));
  const workerDigest = createHash('sha256').update(readFileSync(cli)).digest('hex');
  const provenance = messages.filter(row => row.reason === 'build-script-executed' && row.package_id.includes('domios-server'));
  if (provenance.length !== 1) throw new Error('Expected one exact server build-script receipt');
  const environment = Object.fromEntries(provenance[0].env);
  if (environment.BUILD_SHA !== build || environment.SERVER_CLI_SHA256 !== workerDigest) throw new Error('Server build or CLI binding differs from staged artifacts');
  const contract = readFileSync(path.join(repo, 'src-tauri/crates/codeterm-types/src/server_bootstrap.rs'), 'utf8');
  const constant = name => {
    const match = contract.match(new RegExp(`pub const ${name}: u32 = ([0-9]+);`));
    if (!match) throw new Error(`Missing shared bootstrap contract ${name}`);
    return Number(match[1]);
  };
  if (checkOnly) return { executable: server.executable, worker_cli_sha256: workerDigest };
  if (clientFile !== builtClient) copyResource(builtClient, clientFile);
  if (createHash('sha256').update(readFileSync(clientFile)).digest('hex') !== createHash('sha256').update(readFileSync(builtClient)).digest('hex')) throw new Error('Staged client differs from the built native artifact');
  copyResource(cli, path.join(root, 'cli', path.basename(cli)));
  copyResource(cli, path.join(root, 'cli', bundledCliName(env, platform)));
  if (platform === 'win32') copyResource(path.join(repo, 'src-tauri/conpty'), path.join(root, 'conpty'));
  const stagedServer = path.join(repo, 'target/domios-package/domios-server', path.basename(server.executable));
  copyResource(regular(server.executable), stagedServer);
  const signing = mac ? macDevSigning(env) : null;
  if (mac) signMacDevServer(stagedServer, signing, signingRun);
  const { createManifest } = await import('./package-server-component-manifest.mjs');
  const manifestFile = await createManifest({ cwd: repo, serverFile: stagedServer, clientFile,
    clientIdentity: signing?.clientIdentity, serverBuild: build, serverExecutable: serverRelative, apiVersion: constant('API_VERSION'), bootstrapProtocol: constant('BOOTSTRAP_PROTOCOL') });
  if (!serverOnly) syncBuildDirectory(path.join(repo, 'dist'), path.join(root, 'dist'));
  for (const name of ['skills', 'LICENSE', 'NOTICE']) copyResource(path.join(repo, name), path.join(root, name));
  publishDevExecutable(stagedServer, path.join(root, serverRelative));
  const sourceManifest = path.join(repo, 'target/domios-package/server-component.json');
  const temporary = path.join(root, 'server-component.json.partial');
  copyResource(sourceManifest, temporary);
  renameSync(temporary, path.join(root, 'server-component.json'));
  if (mac) sealMacDevBundle({ root, repo, signing, run: signingRun });
  return { manifest: JSON.parse(readFileSync(manifestFile, 'utf8')), worker_cli_sha256: workerDigest, resource_root: root };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (['stage-dev', 'stage-dev-server', 'check-dev-server'].includes(process.argv[2])) {
    const [cargoLog, treeLog, cliLog, repo, resourceRoot, client, build] = process.argv.slice(3);
    await stageDevResources({ cargoLog, treeLog, cliLog, repo, resourceRoot, client, build, serverOnly: process.argv[2] !== 'stage-dev', checkOnly: process.argv[2] === 'check-dev-server' });
  } else if (process.argv[2] === 'stage-resources') {
    const [cliLog, repo, output] = process.argv.slice(3);
    process.stdout.write(stageResources(cliLog, repo, output));
  } else {
    const [cargoLog, treeLog, build, target, resources, output] = process.argv.slice(2);
    if (!output) throw new Error('Expected Cargo log, closure log, build, target, resources and receipt path');
    const receipt = artifactReceipt({ cargoLog, treeLog, build, target, resources });
    const worker = path.join(resources, 'cli', `codeterm${process.platform === 'win32' ? '.exe' : ''}`);
    receipt.worker_cli_sha256 = createHash('sha256').update(readFileSync(worker)).digest('hex');
    writeFileSync(output, JSON.stringify(receipt, null, 2));
  }
}
