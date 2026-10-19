#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { copyFile, lstat, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HEADLESS_TARGETS, headlessAssetNames } from './headless-release-package.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const PLATFORMS = Object.keys(HEADLESS_TARGETS).sort();
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
function identity(tag, platform, sourceSha) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag) || !PLATFORMS.includes(platform) || !/^[a-f0-9]{40}$/.test(sourceSha)) {
    throw new Error('invalid dry-run evidence identity');
  }
}
async function regular(file) {
  const info = await lstat(file);
  if (!info.isFile() || info.isSymbolicLink() || !info.size) throw new Error('missing regular evidence file: ' + file);
  return readFile(file);
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
function store(verb, key, destination) {
  const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
  execFileSync(bash, ['scripts/ci-artifact.sh', verb, key, destination.replaceAll('\\', '/')],
    { cwd: root, stdio: 'inherit', timeout: 300000 });
}
function verify(manifest, directory, version, platforms, pubkey) {
  invoke('verify-updater-manifest.mjs', ['--manifest', manifest, '--local', directory,
    '--require-version', version, ...platforms.flatMap(platform => ['--require-platform', platform]),
    ...(pubkey ? ['--pubkey', pubkey] : [])]);
}

export async function capture({ tag, platform, sourceSha, artifactDirectory, headlessDirectory, outputDirectory, pubkey }) {
  identity(tag, platform, sourceSha);
  const metadataName = headlessAssetNames(tag, platform).manifest;
  const metadata = JSON.parse((await regular(path.join(headlessDirectory, metadataName))).toString('utf8'));
  if (metadata.schema_version !== 1 || metadata.variant !== 'headless' || metadata.version !== tag.slice(1) ||
      metadata.target !== HEADLESS_TARGETS[platform].target || metadata.source_sha !== sourceSha || metadata.tag !== tag || metadata.platform !== platform) {
    throw new Error('headless provenance differs from dry-run evidence identity');
  }
  const version = tag.slice(1);
  const all = await files(artifactDirectory);
  const wanted = platform.startsWith('darwin-')
    ? [`CodeTerm_${version}_${platform.endsWith('aarch64') ? 'aarch64' : 'x64'}.app.tar.gz`]
    : platform.startsWith('windows-') ? [`CodeTerm_${version}_${platform.endsWith('aarch64') ? 'arm64' : 'x64'}-setup.exe`]
      : [`CodeTerm_${version}_${platform.endsWith('aarch64') ? 'aarch64' : 'amd64'}.AppImage`,
        `CodeTerm_${version}_${platform.endsWith('aarch64') ? 'arm64' : 'amd64'}.deb`];
  await mkdir(outputDirectory, { recursive: true });
  if ((await readdir(outputDirectory)).length) throw new Error('dry-run capture output must be empty');
  const names = [];
  for (const [index, pattern] of wanted.entries()) {
    const matches = all.filter(file => path.basename(file) === pattern);
    if (matches.length !== 1) throw new Error(`expected one ${platform} updater artifact; found ${matches.length}`);
    const source = matches[0];
    const name = platform.startsWith('darwin-')
      ? `CodeTerm_${platform.endsWith('aarch64') ? 'aarch64' : 'x64'}.app.tar.gz` : path.basename(source);
    await regular(source);
    const signature = (await regular(source + '.sig')).toString('utf8').trim();
    await copyFile(source, path.join(outputDirectory, name));
    const fragment = `updater-${platform}${index ? '-deb' : ''}.json`;
    await writeFile(path.join(outputDirectory, fragment), JSON.stringify({ version, notes: `Domios ${version}`,
      pub_date: new Date().toISOString(), platforms: { [platform]: {
        url: `https://github.com/rollacode/codeterm-releases/releases/download/${tag}/${name}`, signature,
      } } }));
    verify(path.join(outputDirectory, fragment), outputDirectory, version, [platform], pubkey);
    names.push(name, fragment);
  }
  const inventory = [];
  for (const name of names.sort()) {
    const bytes = await regular(path.join(outputDirectory, name));
    inventory.push({ name, size: bytes.length, sha256: sha256(bytes) });
  }
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
    for (const platform of PLATFORMS) {
      identity(tag, platform, sourceSha);
      const directory = path.join(inputDirectory, platform);
      const receipt = JSON.parse((await regular(path.join(directory, 'dry-run-evidence.json'))).toString('utf8'));
      if (receipt.schema !== 1 || receipt.tag !== tag || receipt.platform !== platform || receipt.source_sha !== sourceSha ||
          !Array.isArray(receipt.files)) throw new Error('foreign dry-run platform receipt: ' + platform);
      const seen = new Set();
      let mainCount = 0;
      let debCount = 0;
      for (const file of receipt.files) {
        if (!/^[A-Za-z0-9_.-]+$/.test(file.name) || file.name.includes('..') || seen.has(file.name)) throw new Error('unsafe/duplicate evidence path');
        seen.add(file.name);
        const bytes = await regular(path.join(directory, file.name));
        if (file.size !== bytes.length || file.sha256 !== sha256(bytes)) throw new Error('dry-run evidence digest mismatch: ' + file.name);
        if (file.name === `updater-${platform}.json` || file.name === `updater-${platform}-deb.json`) {
          const fragment = JSON.parse(bytes.toString('utf8'));
          if (fragment.version !== tag.slice(1) || JSON.stringify(Object.keys(fragment.platforms ?? {})) !== JSON.stringify([platform])) {
            throw new Error('foreign updater fragment: ' + file.name);
          }
          const isDeb = file.name.endsWith('-deb.json');
          isDeb ? debCount++ : mainCount++;
          await writeFile(path.join(isDeb ? deb : main, file.name), bytes);
        } else {
          if (await lstat(path.join(payloads, file.name)).then(() => true, () => false)) throw new Error('duplicate cross-platform artifact basename');
          await copyFile(path.join(directory, file.name), path.join(payloads, file.name));
        }
      }
      if (mainCount !== 1 || debCount !== (platform.startsWith('linux-') ? 1 : 0)) throw new Error('incomplete platform fragments: ' + platform);
      const actualNames = (await readdir(directory)).sort();
      if (JSON.stringify(actualNames) !== JSON.stringify(['dry-run-evidence.json', ...seen].sort())) throw new Error('unlisted dry-run evidence files');
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
      for (const file of await readdir(outputDirectory)) store('put', `${runId}/release-dry-run/${platform}/${file}`, path.join(outputDirectory, file));
    } else if (verb === 'aggregate') {
      const [inputDirectory, outputDirectory] = args;
      for (const platform of PLATFORMS) store('get-prefix', `${runId}/release-dry-run/${platform}/`, path.join(inputDirectory, platform));
      await aggregate({ tag, sourceSha, inputDirectory, outputDirectory });
    } else throw new Error('unknown dry-run manifest operation');
  } catch (error) { console.error('local dry-run manifest refused: ' + error.message); process.exitCode = 1; }
}
