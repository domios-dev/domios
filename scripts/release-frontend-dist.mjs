#!/usr/bin/env node
// Caller: CI release.yml (frontend-dist), release-macos-build.yml, release-linux.yml, release-linux-arm64.yml, release-windows.yml
// The platform-independent frontend is built once per release run; every platform job installs those exact bytes.
//
//   release-frontend-dist.mjs publish <dist-dir> <archive> <run-id>   packs, stores, prints the SHA-256
//   release-frontend-dist.mjs fetch <run-id> <sha256> <archive>       fetches, verifies, exports it to later steps
//   release-frontend-dist.mjs install <archive> <sha256> <dist-dir>
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createReadStream, existsSync } from 'node:fs';
import { appendFile, mkdir, rm, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { store } from './release-dry-run-manifest.mjs';

const key = runId => `${runId}/frontend-dist/frontend-dist.tar`;

export async function sha256(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

// Git's GNU tar reads "C:" in an archive path as a remote host; Windows ships bsdtar in System32.
function tar(args, cwd) {
  const program = process.platform === 'win32' ? path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe') : 'tar';
  execFileSync(program, args, { cwd, stdio: 'inherit' });
}

export async function pack(distDirectory, archive, run = tar) {
  if (!existsSync(path.join(distDirectory, 'index.html'))) throw new Error('frontend bundle has no index.html: ' + distDirectory);
  await rm(archive, { force: true });
  run(['-cf', path.resolve(archive), '.'], distDirectory);
  return sha256(archive);
}

export async function install(archive, expected, distDirectory, run = tar) {
  if (!/^[0-9a-f]{64}$/.test(expected ?? '')) throw new Error('frontend bundle digest is required');
  const actual = await sha256(archive);
  if (actual !== expected) throw new Error(`frontend bundle digest ${actual} differs from the run's ${expected}`);
  await rm(distDirectory, { recursive: true, force: true });
  await mkdir(distDirectory, { recursive: true });
  run(['-xf', path.resolve(archive)], distDirectory);
  if (!existsSync(path.join(distDirectory, 'index.html'))) throw new Error('installed frontend bundle has no index.html');
}

export async function publish(distDirectory, archive, runId, { put = process.env.GITHUB_ACTIONS === 'true' ? () => {} : store, run = tar } = {}) {
  const digest = await pack(distDirectory, archive, run);
  put('put', key(runId), archive, { bytes: (await stat(archive)).size });
  return digest;
}

export async function fetchArchive(runId, expected, archive, { get = store, env = process.env } = {}) {
  get('get', key(runId), archive, { bytes: 64 * 1024 * 1024 });
  const actual = await sha256(archive);
  if (actual !== expected) throw new Error(`fetched frontend bundle digest ${actual} differs from the run's ${expected}`);
  if (env.GITHUB_ENV) await appendFile(env.GITHUB_ENV, `CODETERM_FRONTEND_ARCHIVE=${archive}\nCODETERM_FRONTEND_SHA256=${expected}\n`);
}

export async function fetchRunArtifact(runId, expected, archive, { env = process.env } = {}) {
  if (!/^[1-9]\d*$/.test(runId ?? '') || path.basename(archive) !== 'frontend-dist.tar') {
    throw new Error('invalid frontend run artifact identity');
  }
  // The workflow downloads the digest-addressed artifact with the pinned artifact action.
  await fetchArchive(runId, expected, archive, { get: () => {}, env });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [verb, ...args] = process.argv.slice(2);
  try {
    if (verb === 'publish') console.log(await publish(args[0], args[1], args[2]));
    else if (verb === 'fetch') await fetchArchive(args[0], args[1], args[2]);
    else if (verb === 'fetch-run-artifact') await fetchRunArtifact(args[0], args[1], args[2]);
    else if (verb === 'install') await install(args[0], args[1], args[2]);
    else throw new Error('unknown frontend bundle operation');
  } catch (error) { console.error('frontend bundle refused: ' + error.message); process.exitCode = 1; }
}
