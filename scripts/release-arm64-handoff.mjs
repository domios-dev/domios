import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createReadStream, realpathSync } from 'node:fs';
import { chmod, lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertMachine } from './release-bundle-smoke.mjs';
import { produceVerifiedCommonStage, releaseCliDigest, verifyServerBuild } from './release-stage.mjs';

const targets = new Set(['aarch64-unknown-linux-gnu', 'aarch64-pc-windows-msvc']);
const finalPackage = /\.(deb|AppImage)$|-setup\.exe$/;
// Windows tar fails GetVolumePathName on the gateway's subst drive, so it runs from the real path.
const tar = args => execFileSync(tarProgram, args, { stdio: 'inherit', timeout: 120000, cwd: realpathSync.native('.') });
const tarProgram = process.platform === 'win32' ? path.join(process.env.SystemRoot, 'System32', 'tar.exe') : 'tar';

async function fileDigest(file) {
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(file)) hash.update(bytes);
  return hash.digest('hex');
}

export async function inventory(directory, prefix = '') {
  const rows = [];
  for (const name of (await readdir(directory)).sort()) {
    const relative = prefix ? `${prefix}/${name}` : name;
    const file = path.join(directory, name);
    const stat = await lstat(file);
    if (stat.isSymbolicLink()) throw new Error('Handoff contains a link: ' + relative);
    if (stat.isDirectory()) rows.push(...await inventory(file, relative));
    else if (stat.isFile()) rows.push({ path: relative, sha256: await fileDigest(file) });
    else throw new Error('Handoff contains a special file: ' + relative);
  }
  return rows;
}

export function verifyReceipt(receipt, expected, actual) {
  if (receipt.schema !== 1 || !targets.has(expected.target) || !/^[a-f0-9]{40}$/.test(expected.source_sha) ||
      !/^v\d+\.\d+\.\d+$/.test(expected.tag) || !/^[1-9]\d*$/.test(expected.run_id) || !/^[1-9]\d*$/.test(expected.run_attempt)) {
    throw new Error('Invalid handoff identity');
  }
  for (const key of ['target', 'source_sha', 'tag', 'run_id', 'run_attempt']) {
    if (receipt[key] !== expected[key]) throw new Error('Foreign handoff ' + key);
  }
  if (!actual.length || JSON.stringify(receipt.files) !== JSON.stringify(actual)) throw new Error('Handoff payload digest inventory differs');
  if (actual.some(row => !/^[a-f0-9]{64}$/.test(row.sha256) || row.path.split('/').some(part => !part || part === '..' || part === '.') || row.path.includes('\\') || row.path.includes(':'))) {
    throw new Error('Invalid handoff payload path or digest');
  }
  if (actual.some(row => /\.sig$/.test(row.path))) throw new Error('Hosted handoff must be unsigned');
  return receipt;
}

function identity(target, tag, sourceSha) {
  return { target, tag, source_sha: sourceSha, run_id: process.env.GITHUB_RUN_ID, run_attempt: process.env.GITHUB_RUN_ATTEMPT };
}

// Only finished packages travel; bundler staging trees such as the AppDir hold links.
export async function bundlePackages(bundleRoot) {
  const packages = [];
  for (const kind of (await readdir(bundleRoot)).sort()) {
    const directory = `${bundleRoot}/${kind}`;
    if (!(await lstat(directory)).isDirectory()) continue;
    for (const name of (await readdir(directory)).sort()) {
      if (finalPackage.test(name) && (await lstat(`${directory}/${name}`)).isFile()) packages.push(`${directory}/${name}`);
    }
  }
  return packages;
}

async function payloadInventory(target) {
  const packages = await bundlePackages(`src-tauri/target/${target}/release/bundle`);
  const directories = ['target/domios-package', `release-artifacts/build-logs/${target}`];
  const rows = [];
  for (const root of directories) rows.push(...await inventory(root, root));
  for (const file of packages) rows.push({ path: file, sha256: await fileDigest(file) });
  rows.sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
  return { roots: [...directories, ...packages], rows };
}

export async function pack(target, tag, sourceSha, archive) {
  if (!targets.has(target)) throw new Error('Unsupported hosted target');
  const cliHash = await releaseCliDigest({ target });
  const server = await verifyServerBuild({ target, sourceSha, expectedCliSha256: cliHash,
    cargoLog: `release-artifacts/build-logs/${target}/server.jsonl`, treeLog: `release-artifacts/build-logs/${target}/closure.txt` });
  const staged = `target/domios-package/domios-server/domios-server${target.includes('windows') ? '.exe' : ''}`;
  if (await fileDigest(server) !== await fileDigest(staged)) throw new Error('Transfer server differs from compiled server');
  const stagedCli = `target/domios-package/cli/codeterm${target.includes('windows') ? '.exe' : ''}`;
  if (await fileDigest(stagedCli) !== cliHash) throw new Error('Transfer CLI differs from the compiled server binding');
  const { roots, rows } = await payloadInventory(target);
  const receipt = { schema: 1, ...identity(target, tag, sourceSha), files: rows };
  verifyReceipt(receipt, identity(target, tag, sourceSha), rows);
  await writeFile('release-arm64-handoff.json', JSON.stringify(receipt, null, 2) + '\n');
  await mkdir(path.dirname(path.resolve(archive)), { recursive: true });
  tar(['-cf', path.resolve(archive), 'release-arm64-handoff.json', ...roots]);
  const hash = await fileDigest(archive);
  if (process.env.GITHUB_OUTPUT) await writeFile(process.env.GITHUB_OUTPUT, `sha256=${hash}\n`, { flag: 'a' });
  return hash;
}

export async function unpack(target, tag, sourceSha, archive, expectedDigest) {
  if (!targets.has(target) || !/^[a-f0-9]{64}$/.test(expectedDigest ?? '') || await fileDigest(archive) !== expectedDigest) {
    throw new Error('Handoff archive digest differs');
  }
  tar(['-xf', path.resolve(archive)]);
  const receipt = JSON.parse(await readFile('release-arm64-handoff.json', 'utf8'));
  const { rows } = await payloadInventory(target);
  verifyReceipt(receipt, identity(target, tag, sourceSha), rows);
  const windows = target.includes('windows');
  const suffix = windows ? '.exe' : '';
  const stage = 'target/domios-package';
  const server = `${stage}/domios-server/domios-server${suffix}`;
  const cli = `${stage}/cli/codeterm${suffix}`;
  for (const file of [server, cli]) assertMachine(await readFile(file), 'arm64', windows ? 'PE' : 'ELF');
  const bundleRoot = `src-tauri/target/${target}/release/bundle`;
  const bundles = rows.filter(row => row.path.startsWith(bundleRoot + '/') && (windows ? /-setup\.exe$/ : /\.deb$/).test(row.path));
  if (bundles.length !== 1) throw new Error('Expected one full package in handoff');
  if (!windows) for (const row of rows.filter(row => /\.AppImage$/.test(row.path))) await chmod(row.path, 0o755);
  const result = await produceVerifiedCommonStage({ target, tag, version: tag.slice(1), sourceSha,
    bundle: bundles[0].path, server, cliHash: await fileDigest(cli), outputDirectory: 'release-artifacts' });
  await writeFile(`release-artifacts/hosted-handoff-${target}.json`, JSON.stringify({ ...receipt, archive_sha256: expectedDigest }, null, 2) + '\n');
  return result;
}

export async function verifyTransferred(target, tag, sourceSha, copies) {
  const receipt = JSON.parse(await readFile('release-arm64-handoff.json', 'utf8'));
  const { rows } = await payloadInventory(target);
  verifyReceipt(receipt, identity(target, tag, sourceSha), rows.filter(row => !row.path.endsWith('.sig')));
  if (copies) {
    for (const row of receipt.files.filter(row => /\.(AppImage|deb)$/.test(row.path))) {
      if (await fileDigest(path.join(copies, path.basename(row.path))) !== row.sha256) throw new Error('Signing copy differs from hosted bundle');
    }
  }
  if (process.env.GITHUB_STEP_SUMMARY) await writeFile(process.env.GITHUB_STEP_SUMMARY,
    `Hosted unsigned ${target}: complete payload SHA-256 inventory verified before/after detached signing; source ${sourceSha}.\n`, { flag: 'a' });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [verb, ...args] = process.argv.slice(2);
  try {
    if (verb === 'pack') await pack(...args);
    else if (verb === 'unpack') await unpack(...args);
    else if (verb === 'verify') await verifyTransferred(...args);
    else throw new Error('Expected pack or unpack');
  } catch (error) { process.stderr.write(error.message + '\n'); process.exitCode = 1; }
}
