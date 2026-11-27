import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { chmod, cp, lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { HEADLESS_TARGETS, packageHeadless } from '../headless-release-package.mjs';
import { assertServerClosure, serverArtifact, workerArtifact } from '../server-artifact.mjs';
import { SUPPORTED_API_VERSION, SUPPORTED_BOOTSTRAP_PROTOCOL, validateManifest } from '../package-server-component-manifest.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const messages = async file => (await readFile(file, 'utf8')).split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));

export function selectQaArtifact(rows, name, source) {
  const matches = rows.filter(
    row => row.reason === 'compiler-artifact' && row.target?.name === name && row.target.kind?.includes('bin') && row.executable && !row.profile?.test,
  );
  if (!rows.some(row => row.reason === 'build-finished' && row.success) || matches.length !== 1) {
    throw new Error(`Expected one successful QA ${name} binary artifact`);
  }
  const binary = matches[0];
  if (binary.profile?.debug_assertions !== true || path.resolve(binary.target.src_path) !== path.resolve(source)) {
    throw new Error(`Expected debug_assertions=true QA binary for ${name}`);
  }
  return binary;
}

export async function stripBinaryWithDebugSymbols({
  source,
  strippedPath,
  debugSymbolsPath,
  objcopy = process.env.OBJCOPY || 'objcopy',
  run = execFileSync,
}) {
  const info = await lstat(source);
  if (!info.isFile() || info.isSymbolicLink() || info.size === 0) {
    throw new Error(`Expected a non-empty regular binary to strip: ${source}`);
  }
  await mkdir(path.dirname(strippedPath), { recursive: true });
  if (debugSymbolsPath) {
    await mkdir(path.dirname(debugSymbolsPath), { recursive: true });
    run(objcopy, ['--only-keep-debug', source, debugSymbolsPath], { stdio: 'inherit' });
  }
  run(objcopy, ['--strip-debug', '--strip-unneeded', source, strippedPath], { stdio: 'inherit' });
  if (process.platform !== 'win32') {
    await chmod(strippedPath, 0o755);
  }
  const strippedBytes = await readFile(strippedPath);
  if (!strippedBytes.length) {
    throw new Error(`Stripped binary is empty: ${strippedPath}`);
  }
  return {
    source,
    strippedPath,
    debugSymbolsPath: debugSymbolsPath ?? null,
    originalBytes: info.size,
    strippedBytes: strippedBytes.length,
    sha256: hash(strippedBytes),
  };
}

async function cliPath(file, repoRoot = process.cwd()) {
  const rows = await messages(file);
  selectQaArtifact(rows, 'codeterm', path.join(repoRoot, 'cli/src/main.rs'));
  return workerArtifact(rows, repoRoot);
}

export async function prepareQaCliDigest({
  cliLog,
  target,
  repoRoot = process.cwd(),
  strippedRoot = path.resolve(repoRoot, 'target/cloud-qa/stripped'),
  debugSymbolsRoot = path.resolve(repoRoot, 'target/cloud-qa/debug-symbols'),
  objcopy = process.env.OBJCOPY || 'objcopy',
  run = execFileSync,
}) {
  if (!target) throw new Error('Expected a Linux target for QA CLI stripping');
  const worker = await cliPath(cliLog, repoRoot);
  return stripBinaryWithDebugSymbols({
    source: worker,
    strippedPath: path.join(strippedRoot, target, 'codeterm'),
    debugSymbolsPath: path.join(debugSymbolsRoot, target, 'codeterm.debug'),
    objcopy,
    run,
  });
}

export async function packageCloudQa({
  target,
  logs,
  repoRoot = process.cwd(),
  stageRoot = path.resolve(repoRoot, 'target/cloud-qa/stage'),
  outputDir = path.resolve(repoRoot, 'target/cloud-qa/archives'),
  strippedRoot = path.resolve(repoRoot, 'target/cloud-qa/stripped'),
  debugSymbolsRoot = path.resolve(repoRoot, 'target/cloud-qa/debug-symbols'),
  sourceSha = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim(),
  version,
  objcopy = process.env.OBJCOPY || 'objcopy',
  run = execFileSync,
}) {
  const metadata = Object.entries(HEADLESS_TARGETS).find(([platform, row]) => platform.startsWith('linux-') && row.target === target);
  if (!metadata) throw new Error('Expected a Linux headless target');
  const resolvedVersion = version ?? JSON.parse(await readFile(path.join(repoRoot, 'src-tauri/tauri.conf.json'), 'utf8')).version;
  const serverRows = await messages(path.join(logs, 'server.jsonl'));
  const binary = selectQaArtifact(serverRows, 'domios-server', path.join(repoRoot, 'src-tauri/crates/domios-server/src/main.rs'));
  if (JSON.stringify([...binary.features].sort()) !== JSON.stringify(['embeddings', 'mem-postgres'])) {
    throw new Error('Unexpected QA server features');
  }
  const rawServer = serverArtifact(serverRows);
  const strippedCli = await prepareQaCliDigest({
    cliLog: path.join(logs, 'cli.jsonl'),
    target,
    repoRoot,
    strippedRoot,
    debugSymbolsRoot,
    objcopy,
    run,
  });
  const strippedServer = await stripBinaryWithDebugSymbols({
    source: rawServer,
    strippedPath: path.join(strippedRoot, target, 'domios-server'),
    debugSymbolsPath: path.join(debugSymbolsRoot, target, 'domios-server.debug'),
    objcopy,
    run,
  });
  const receipts = serverRows.filter(row => row.reason === 'build-script-executed' && row.package_id?.includes('domios-server'));
  if (receipts.length !== 1) throw new Error('Missing exact QA server build receipt');
  const environment = Object.fromEntries(receipts[0].env ?? []);
  if (environment.BUILD_SHA !== sourceSha || environment.SERVER_CLI_SHA256 !== strippedCli.sha256) {
    throw new Error('QA server source SHA or CLI digest mismatch');
  }
  assertServerClosure((await readFile(path.join(logs, 'closure.txt'), 'utf8')).split(/\r?\n/).filter(Boolean));
  const stage = path.resolve(stageRoot, target);
  await mkdir(stage, { recursive: true });
  if ((await readdir(stage)).length) throw new Error('Cloud QA package requires a fresh empty stage');
  for (const [source, relative] of [
    [strippedServer.strippedPath, 'domios-server/domios-server'],
    [strippedCli.strippedPath, 'cli/codeterm'],
    [strippedCli.strippedPath, 'cli/domios'],
    ...['dist', 'skills', 'LICENSE', 'NOTICE'].map(name => [path.join(repoRoot, name), name]),
  ]) {
    await mkdir(path.dirname(path.join(stage, relative)), { recursive: true });
    await cp(source, path.join(stage, relative), { recursive: true, errorOnExist: true, force: false });
  }
  const stagedServerSha256 = hash(await readFile(path.join(stage, 'domios-server/domios-server')));
  if (stagedServerSha256 !== strippedServer.sha256) {
    throw new Error('Staged QA server bytes differ from stripped binary');
  }
  const manifest = validateManifest({
    schema: 2,
    server_build: sourceSha,
    server_executable: 'domios-server/domios-server',
    server_sha256: stagedServerSha256,
    client_identity: { kind: 'none' },
    api_version: SUPPORTED_API_VERSION,
    bootstrap_protocol: SUPPORTED_BOOTSTRAP_PROTOCOL,
  });
  await writeFile(path.join(stage, 'server-component.json'), JSON.stringify(manifest) + '\n');
  const files = [];
  async function inventory(directory, prefix = '') {
    for (const name of await readdir(directory)) {
      const relative = prefix ? `${prefix}/${name}` : name;
      const absolute = path.join(directory, name);
      const info = await lstat(absolute);
      if (info.isSymbolicLink()) throw new Error('Symlinks refused in cloud QA payload');
      if (info.isDirectory()) await inventory(absolute, relative);
      else if (info.isFile()) files.push({ path: relative, sha256: hash(await readFile(absolute)) });
      else throw new Error('Non-regular file refused in cloud QA payload');
    }
  }
  await inventory(stage);
  files.sort((left, right) => Buffer.compare(Buffer.from(left.path), Buffer.from(right.path)));
  await writeFile(
    path.join(stage, 'headless-stage.json'),
    JSON.stringify({ schema_version: 1, platform: metadata[0], target, tag: `v${resolvedVersion}`, version: resolvedVersion, source_sha: sourceSha, files }) + '\n',
  );
  const result = await packageHeadless({
    stageDirectory: stage,
    outputDirectory: path.resolve(outputDir),
    target,
    tag: `v${resolvedVersion}`,
    version: resolvedVersion,
    sourceSha,
  });
  return {
    candidate_qa: true,
    version: resolvedVersion,
    source_sha: sourceSha,
    archive: result.archivePath,
    sha256: result.archive_sha256,
    server_sha256: stagedServerSha256,
    cli_sha256: strippedCli.sha256,
    server_bytes: strippedServer.strippedBytes,
    server_unstripped_bytes: strippedServer.originalBytes,
    cli_bytes: strippedCli.strippedBytes,
    cli_unstripped_bytes: strippedCli.originalBytes,
    debug_symbols: {
      server: strippedServer.debugSymbolsPath,
      cli: strippedCli.debugSymbolsPath,
    },
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.env.CI !== 'true' || process.env.DOMIOS_CLOUD_CANDIDATE !== '1') {
    throw new Error('Cloud QA candidate mode must be explicit');
  }
  const [action, value, third] = process.argv.slice(2);
  if (action === 'cli-digest') {
    const target = third || process.env.CARGO_BUILD_TARGET;
    const prepared = await prepareQaCliDigest({ cliLog: value, target });
    console.log(prepared.sha256);
  } else if (action === 'package') {
    const summary = await packageCloudQa({ target: value, logs: third });
    console.log(JSON.stringify(summary));
  } else {
    throw new Error('Expected cli-digest or package');
  }
}
