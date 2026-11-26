import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { cp, lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
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

async function cliPath(file) {
  const rows = await messages(file);
  selectQaArtifact(rows, 'codeterm', 'cli/src/main.rs');
  return workerArtifact(rows, process.cwd());
}

async function packageCloudQa(target, logs) {
  const metadata = Object.entries(HEADLESS_TARGETS).find(([platform, row]) => platform.startsWith('linux-') && row.target === target);
  if (!metadata) throw new Error('Expected a Linux headless target');
  const sourceSha = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const version = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8')).version;
  const serverRows = await messages(path.join(logs, 'server.jsonl'));
  const binary = selectQaArtifact(serverRows, 'domios-server', 'src-tauri/crates/domios-server/src/main.rs');
  if (JSON.stringify([...binary.features].sort()) !== JSON.stringify(['embeddings', 'mem-postgres'])) {
    throw new Error('Unexpected QA server features');
  }
  const server = serverArtifact(serverRows);
  const worker = await cliPath(path.join(logs, 'cli.jsonl'));
  const workerDigest = hash(await readFile(worker));
  const receipts = serverRows.filter(row => row.reason === 'build-script-executed' && row.package_id?.includes('domios-server'));
  if (receipts.length !== 1) throw new Error('Missing exact QA server build receipt');
  const environment = Object.fromEntries(receipts[0].env ?? []);
  if (environment.BUILD_SHA !== sourceSha || environment.SERVER_CLI_SHA256 !== workerDigest) {
    throw new Error('QA server source SHA or CLI digest mismatch');
  }
  assertServerClosure((await readFile(path.join(logs, 'closure.txt'), 'utf8')).split(/\r?\n/).filter(Boolean));
  const stage = path.resolve('target/cloud-qa/stage', target);
  await mkdir(stage, { recursive: true });
  if ((await readdir(stage)).length) throw new Error('Cloud QA package requires a fresh empty stage');
  for (const [source, relative] of [
    [server, 'domios-server/domios-server'],
    [worker, 'cli/codeterm'],
    [worker, 'cli/domios'],
    ...['dist', 'skills', 'LICENSE', 'NOTICE'].map(name => [name, name]),
  ]) {
    await mkdir(path.dirname(path.join(stage, relative)), { recursive: true });
    await cp(source, path.join(stage, relative), { recursive: true, errorOnExist: true, force: false });
  }
  const manifest = validateManifest({
    schema: 2,
    server_build: sourceSha,
    server_executable: 'domios-server/domios-server',
    server_sha256: hash(await readFile(server)),
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
    JSON.stringify({ schema_version: 1, platform: metadata[0], target, tag: `v${version}`, version, source_sha: sourceSha, files }) + '\n',
  );
  const result = await packageHeadless({
    stageDirectory: stage,
    outputDirectory: path.resolve('target/cloud-qa/archives'),
    target,
    tag: `v${version}`,
    version,
    sourceSha,
  });
  console.log(JSON.stringify({ candidate_qa: true, version, source_sha: sourceSha, archive: result.archivePath, sha256: result.archive_sha256 }));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.env.CI !== 'true' || process.env.DOMIOS_CLOUD_CANDIDATE !== '1') {
    throw new Error('Cloud QA candidate mode must be explicit');
  }
  const [action, value, logs] = process.argv.slice(2);
  if (action === 'cli-digest') console.log(hash(await readFile(await cliPath(value))));
  else if (action === 'package') await packageCloudQa(value, logs);
  else throw new Error('Expected cli-digest or package');
}
