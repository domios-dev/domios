import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { cp, lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { HEADLESS_TARGETS, packageHeadless } from '../headless-release-package.mjs';
import { assertServerClosure, serverArtifact } from '../server-artifact.mjs';
import { validateManifest } from '../package-server-component-manifest.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const messages = async file => (await readFile(file, 'utf8')).split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));

export function selectProductionArtifact(rows, name, source) {
  const matches = rows.filter(row => row.reason === 'compiler-artifact' && row.target?.name === name && row.target.kind?.includes('bin') && row.executable && !row.profile?.test);
  if (!rows.some(row => row.reason === 'build-finished' && row.success) || matches.length !== 1) throw new Error('Expected one successful production binary artifact');
  const binary = matches[0];
  if (binary.profile.debug_assertions !== false || typeof binary.profile.opt_level !== 'string' || binary.profile.opt_level === '0' || path.resolve(binary.target.src_path) !== path.resolve(source)) throw new Error('Development or foreign binary refused');
  return binary;
}

async function artifact(file, name, source) {
  const rows = await messages(file);
  return { rows, binary: selectProductionArtifact(rows, name, source) };
}

async function cli(file) {
  return (await artifact(file, 'codeterm', 'cli/src/main.rs')).binary.executable;
}

async function packageCi(target, logs) {
  const metadata = Object.entries(HEADLESS_TARGETS).find(([platform, row]) => platform.startsWith('linux-') && row.target === target);
  if (!metadata) throw new Error('Expected a Linux headless target');
  const sourceSha = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const version = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8')).version;
  const { rows, binary } = await artifact(path.join(logs, 'server.jsonl'), 'domios-server', 'src-tauri/crates/domios-server/src/main.rs');
  if (JSON.stringify([...binary.features].sort()) !== JSON.stringify(['embeddings', 'mem-postgres'])) throw new Error('Unexpected server features');
  const server = serverArtifact(rows);
  const worker = await cli(path.join(logs, 'cli.jsonl'));
  const workerDigest = hash(await readFile(worker));
  const receipts = rows.filter(row => row.reason === 'build-script-executed' && row.package_id.includes('domios-server'));
  if (receipts.length !== 1) throw new Error('Missing exact server build receipt');
  const environment = Object.fromEntries(receipts[0].env);
  if (environment.BUILD_SHA !== sourceSha || environment.SERVER_CLI_SHA256 !== workerDigest) throw new Error('Server source or CLI binding mismatch');
  assertServerClosure((await readFile(path.join(logs, 'closure.txt'), 'utf8')).split(/\r?\n/).filter(Boolean));
  const stage = path.resolve('target/docker-smoke/stage', target);
  await mkdir(stage, { recursive: true });
  if ((await readdir(stage)).length) throw new Error('CI package requires a fresh empty stage');
  for (const [source, relative] of [[server, 'domios-server/domios-server'], [worker, 'cli/codeterm'], [worker, 'cli/domios'], ...['dist', 'skills', 'LICENSE', 'NOTICE'].map(name => [name, name])]) {
    await mkdir(path.dirname(path.join(stage, relative)), { recursive: true });
    await cp(source, path.join(stage, relative), { recursive: true, errorOnExist: true, force: false });
  }
  const contract = await readFile('src-tauri/crates/codeterm-types/src/server_bootstrap.rs', 'utf8');
  const constant = name => {
    const match = contract.match(new RegExp(`pub const ${name}: u32 = ([0-9]+);`));
    if (!match) throw new Error('Missing bootstrap contract constant');
    return Number(match[1]);
  };
  const manifest = validateManifest({ schema: 2, server_build: sourceSha,
    server_executable: 'domios-server/domios-server', server_sha256: hash(await readFile(server)),
    client_identity: { kind: 'none' }, api_version: constant('API_VERSION'), bootstrap_protocol: constant('BOOTSTRAP_PROTOCOL') });
  await writeFile(path.join(stage, 'server-component.json'), JSON.stringify(manifest) + '\n');
  const files = [];
  async function inventory(directory, prefix = '') {
    for (const name of await readdir(directory)) {
      const relative = prefix ? `${prefix}/${name}` : name;
      const absolute = path.join(directory, name);
      const info = await lstat(absolute);
      if (info.isSymbolicLink()) throw new Error('Links refused in CI payload');
      if (info.isDirectory()) await inventory(absolute, relative);
      else if (info.isFile()) files.push({ path: relative, sha256: hash(await readFile(absolute)) });
      else throw new Error('Nonregular CI payload refused');
    }
  }
  await inventory(stage);
  files.sort((left, right) => Buffer.compare(Buffer.from(left.path), Buffer.from(right.path)));
  await writeFile(path.join(stage, 'headless-stage.json'), JSON.stringify({ schema_version: 1,
    platform: metadata[0], target, tag: `v${version}`, version, source_sha: sourceSha, files }) + '\n');
  const result = await packageHeadless({ stageDirectory: stage, outputDirectory: path.resolve('target/docker-smoke/archives'), target, tag: `v${version}`, version, sourceSha });
  console.log(JSON.stringify({ smoke_only: true, archive: result.archivePath, sha256: result.archive_sha256 }));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.env.CI !== 'true' || process.env.DOMIOS_DOCKER_SMOKE !== '1') throw new Error('CI smoke mode must be explicit');
  const [action, value, logs] = process.argv.slice(2);
  if (action === 'cli-digest') console.log(hash(await readFile(await cli(value))));
  else if (action === 'package') await packageCi(value, logs);
  else throw new Error('Expected cli-digest or package');
}
