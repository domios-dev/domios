#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { copyFile, lstat, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HEADLESS_TARGETS, headlessAssetNames } from './headless-release-package.mjs';
import { verifyHeadlessAssets } from './release-headless-assets.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const PLATFORMS = Object.keys(HEADLESS_TARGETS).sort();
function identity(tag, platform, sourceSha) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag) || !PLATFORMS.includes(platform) || !/^[a-f0-9]{40}$/.test(sourceSha)) {
    throw new Error('invalid dry-run evidence identity');
  }
}
async function regularSize(file) {
  const info = await lstat(file);
  if (!info.isFile() || info.isSymbolicLink() || !info.size) throw new Error('missing regular evidence file: ' + file);
  return info.size;
}
async function regular(file) {
  await regularSize(file);
  return readFile(file);
}
async function fingerprint(file) {
  const size = await regularSize(file);
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return { size, sha256: hash.digest('hex') };
}
function evidenceNames(tag, platform) {
  const version = tag.slice(1);
  const arm = platform.endsWith('aarch64');
  const updater = platform.startsWith('darwin-')
    ? [{ source: `CodeTerm_${version}_${arm ? 'aarch64' : 'x64'}.app.tar.gz`, name: `CodeTerm_${arm ? 'aarch64' : 'x64'}.app.tar.gz` }]
    : platform.startsWith('windows-') ? [`CodeTerm_${version}_${arm ? 'arm64' : 'x64'}-setup.exe`]
      : [`CodeTerm_${version}_${arm ? 'aarch64' : 'amd64'}.AppImage`, `CodeTerm_${version}_${arm ? 'arm64' : 'amd64'}.deb`];
  const payloads = updater.map(entry => typeof entry === 'string' ? { source: entry, name: entry } : entry);
  const fragments = payloads.map((_, index) => `updater-${platform}${index ? '-deb' : ''}.json`);
  const headless = headlessAssetNames(tag, platform);
  const installers = platform.startsWith('darwin-') ? [`CodeTerm_${version}_${arm ? 'aarch64' : 'x64'}.dmg`] : [];
  return { payloads, fragments, installers, headless: [headless.archive, headless.checksum, headless.manifest] };
}
function single(all, name, platform) {
  const matches = all.filter(file => path.basename(file) === name);
  if (matches.length !== 1) throw new Error(`expected one ${platform} artifact ${name}; found ${matches.length}`);
  return matches[0];
}
async function files(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await files(full));
    else if (entry.isFile()) result.push(full);
  }
  return result;
}
function invoke(script, args) {
  execFileSync(process.execPath, [path.join(root, 'scripts', script), ...args], { cwd: root, stdio: 'inherit' });
}
const STORE_BASE_MS = 20 * 60_000;
const STORE_FLOOR_BYTES_PER_SECOND = 128 * 1024;
const PLATFORM_EVIDENCE_BYTES = 256 * 1024 * 1024;
export function storeTimeoutMs(bytes = 0) {
  return STORE_BASE_MS + Math.ceil(bytes / STORE_FLOOR_BYTES_PER_SECOND) * 1000;
}
function runStore(verb, key, destination, timeout) {
  const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
  execFileSync(bash, ['scripts/ci-artifact.sh', verb, key, destination.replaceAll('\\', '/')],
    { cwd: root, stdio: 'inherit', timeout });
}
// Only a transfer deadline is retried, once; a store error the transport reports fails at once.
export function store(verb, key, destination, { bytes = 0, run = runStore, now = Date.now, log = console.warn } = {}) {
  const timeout = storeTimeoutMs(bytes);
  for (let attempt = 1; ; attempt++) {
    const started = now();
    try { return run(verb, key, destination, timeout); } catch (error) {
      if (error?.code !== 'ETIMEDOUT' || attempt > 1) throw error;
      log(`::warning::ci-artifact ${verb} ${key} (${bytes} bytes) timed out after ${Math.round((now() - started) / 1000)}s; retrying once`);
    }
  }
}
function verify(manifest, directory, version, platforms, pubkey) {
  invoke('verify-updater-manifest.mjs', ['--manifest', manifest, '--local', directory,
    '--require-version', version, ...platforms.flatMap(platform => ['--require-platform', platform]),
    ...(pubkey ? ['--pubkey', pubkey] : [])]);
}

export async function capture({ tag, platform, sourceSha, artifactDirectory, headlessDirectory, outputDirectory, pubkey }) {
  identity(tag, platform, sourceSha);
  await verifyHeadlessAssets({ tag, platform, directory: headlessDirectory, sourceSha });
  const version = tag.slice(1);
  const all = await files(artifactDirectory);
  const expected = evidenceNames(tag, platform);
  const sources = [
    ...expected.installers.map(name => ({ name, source: single(all, name, platform) })),
    ...expected.headless.map(name => ({ name, source: path.join(headlessDirectory, name) })),
  ];
  await mkdir(outputDirectory, { recursive: true });
  if ((await readdir(outputDirectory)).length) throw new Error('dry-run capture output must be empty');
  const names = [];
  for (const [index, { source: pattern, name }] of expected.payloads.entries()) {
    const source = single(all, pattern, platform);
    await regularSize(source);
    const signature = (await regular(source + '.sig')).toString('utf8').trim();
    await copyFile(source, path.join(outputDirectory, name));
    const fragment = expected.fragments[index];
    await writeFile(path.join(outputDirectory, fragment), JSON.stringify({ version, notes: `Domios ${version}`,
      pub_date: new Date().toISOString(), platforms: { [platform]: {
        url: `https://github.com/rollacode/codeterm-releases/releases/download/${tag}/${name}`, signature,
      } } }));
    verify(path.join(outputDirectory, fragment), outputDirectory, version, [platform], pubkey);
    names.push(name, fragment);
  }
  for (const { name, source } of sources) {
    await regularSize(source);
    await copyFile(source, path.join(outputDirectory, name));
    names.push(name);
  }
  await verifyHeadlessAssets({ tag, platform, directory: outputDirectory, sourceSha });
  const inventory = [];
  for (const name of names.sort()) inventory.push({ name, ...await fingerprint(path.join(outputDirectory, name)) });
  const receipt = { schema: 1, tag, platform, source_sha: sourceSha, files: inventory };
  await writeFile(path.join(outputDirectory, 'dry-run-evidence.json'), JSON.stringify(receipt, null, 2) + '\n');
  return receipt;
}

export async function aggregate({ tag, sourceSha, inputDirectory, outputDirectory, pubkey }) {
  const scratch = await mkdtemp(path.join(os.tmpdir(), 'domios-local-manifest-'));
  try {
    const main = path.join(scratch, 'main');
    const deb = path.join(scratch, 'deb');
    const payloads = path.join(scratch, 'payloads');
    await Promise.all([mkdir(main), mkdir(deb), mkdir(payloads)]);
    const evidence = [];
    const artifactNames = new Set();
    for (const platform of PLATFORMS) {
      identity(tag, platform, sourceSha);
      const directory = path.join(inputDirectory, platform);
      const receipt = JSON.parse((await regular(path.join(directory, 'dry-run-evidence.json'))).toString('utf8'));
      if (receipt.schema !== 1 || receipt.tag !== tag || receipt.platform !== platform || receipt.source_sha !== sourceSha ||
          !Array.isArray(receipt.files)) throw new Error('foreign dry-run platform receipt: ' + platform);
      const expected = evidenceNames(tag, platform);
      const payloadNames = new Set(expected.payloads.map(entry => entry.name));
      const seen = new Set();
      for (const file of receipt.files) {
        if (!/^[A-Za-z0-9_.-]+$/.test(file.name) || file.name.includes('..') || seen.has(file.name)) throw new Error('unsafe/duplicate evidence path');
        seen.add(file.name);
        const actual = await fingerprint(path.join(directory, file.name));
        if (file.size !== actual.size || file.sha256 !== actual.sha256) throw new Error('dry-run evidence digest mismatch: ' + file.name);
        if (expected.fragments.includes(file.name)) {
          const bytes = await regular(path.join(directory, file.name));
          const fragment = JSON.parse(bytes.toString('utf8'));
          if (fragment.version !== tag.slice(1) || JSON.stringify(Object.keys(fragment.platforms ?? {})) !== JSON.stringify([platform])) {
            throw new Error('foreign updater fragment: ' + file.name);
          }
          await writeFile(path.join(file.name.endsWith('-deb.json') ? deb : main, file.name), bytes);
        } else {
          if (artifactNames.has(file.name)) throw new Error('duplicate cross-platform artifact basename');
          artifactNames.add(file.name);
          if (payloadNames.has(file.name)) await copyFile(path.join(directory, file.name), path.join(payloads, file.name));
        }
      }
      const required = [...payloadNames, ...expected.fragments, ...expected.installers, ...expected.headless].sort();
      if (JSON.stringify([...seen].sort()) !== JSON.stringify(required)) throw new Error('incomplete platform evidence: ' + platform);
      const actualNames = (await readdir(directory)).sort();
      if (JSON.stringify(actualNames) !== JSON.stringify(['dry-run-evidence.json', ...seen].sort())) throw new Error('unlisted dry-run evidence files');
      await verifyHeadlessAssets({ tag, platform, directory, sourceSha });
      evidence.push(receipt);
    }
    const latest = path.join(scratch, 'latest.json');
    const latestDeb = path.join(scratch, 'latest-deb.json');
    invoke('merge-latest-json.js', ['--in', main, '--out', latest]);
    invoke('merge-latest-json.js', ['--in', deb, '--out', latestDeb]);
    verify(latest, payloads, tag.slice(1), PLATFORMS, pubkey);
    verify(latestDeb, payloads, tag.slice(1), ['linux-aarch64', 'linux-x86_64'], pubkey);
    await mkdir(outputDirectory, { recursive: true });
    await copyFile(latest, path.join(outputDirectory, 'latest.json'));
    await copyFile(latestDeb, path.join(outputDirectory, 'latest-deb.json'));
    const result = { schema: 1, mode: 'local_dry_run', tag, source_sha: sourceSha, platforms: PLATFORMS, evidence };
    await writeFile(path.join(outputDirectory, 'manifest-local-verification.json'), JSON.stringify(result, null, 2) + '\n');
    return result;
  } finally { await rm(scratch, { recursive: true, force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [verb, tag, sourceSha, runId, ...args] = process.argv.slice(2);
  try {
    if (!/^[1-9]\d*$/.test(runId)) throw new Error('invalid CI evidence run ID');
    if (verb === 'capture') {
      const [platform, artifactDirectory, headlessDirectory, outputDirectory] = args;
      await capture({ tag, sourceSha, platform, artifactDirectory, headlessDirectory, outputDirectory });
      const stored = (await readdir(outputDirectory)).sort((left, right) =>
        Number(left === 'dry-run-evidence.json') - Number(right === 'dry-run-evidence.json'));
      for (const file of stored) {
        const local = path.join(outputDirectory, file);
        store('put', `${runId}/release-dry-run/${platform}/${file}`, local, { bytes: (await lstat(local)).size });
      }
    } else if (verb === 'aggregate') {
      const [inputDirectory, outputDirectory] = args;
      for (const platform of PLATFORMS) {
        store('get-prefix', `${runId}/release-dry-run/${platform}/`, path.join(inputDirectory, platform), { bytes: PLATFORM_EVIDENCE_BYTES });
      }
      await aggregate({ tag, sourceSha, inputDirectory, outputDirectory });
    } else throw new Error('unknown dry-run manifest operation');
  } catch (error) { console.error('local dry-run manifest refused: ' + error.message); process.exitCode = 1; }
}
