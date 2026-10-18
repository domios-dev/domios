#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HEADLESS_TARGETS, headlessAssetNames } from './headless-release-package.mjs';

function fail(message) {
  throw new Error(message);
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export async function verifyHeadlessAssets({ tag, platform, directory, sourceSha }) {
  const metadata = HEADLESS_TARGETS[platform];
  const names = headlessAssetNames(tag, platform);
  const paths = [names.archive, names.checksum, names.manifest].map(name => path.join(directory, name));
  const [archive, checksum, manifestBytes] = await Promise.all([
    readFile(paths[0]), readFile(paths[1], 'utf8'), readFile(paths[2]),
  ]);
  const manifest = JSON.parse(manifestBytes.toString('utf8'));
  const fields = ['schema_version', 'variant', 'platform', 'target', 'tag', 'version', 'source_sha', 'archive', 'checksum', 'archive_sha256'];
  if (JSON.stringify(Object.keys(manifest).sort()) !== JSON.stringify(fields.sort()) ||
      manifest.schema_version !== 1 || manifest.variant !== 'headless' ||
      manifest.platform !== platform || manifest.target !== metadata.target ||
      manifest.tag !== tag || manifest.version !== tag.slice(1) ||
      !/^[a-f0-9]{40}$/.test(manifest.source_sha ?? '') ||
      (sourceSha && manifest.source_sha !== sourceSha) ||
      manifest.archive !== names.archive || manifest.checksum !== names.checksum ||
      manifest.archive_sha256 !== sha256(archive) ||
      checksum !== `${manifest.archive_sha256}  ${names.archive}\n`) {
    fail('headless assets fail platform, source, checksum, or metadata readback');
  }
  return { paths, names, manifest, bytes: [archive, Buffer.from(checksum), manifestBytes] };
}

export async function publishHeadlessAssets({ tag, platform, directory, sourceSha, repository, env = process.env }) {
  if (!repository || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) fail('explicit release repository is required');
  const local = await verifyHeadlessAssets({ tag, platform, directory, sourceSha });
  execFileSync('gh', ['release', 'upload', tag, ...local.paths, '--clobber', '--repo', repository],
    { env, stdio: 'inherit', timeout: 300000 });
  const scratch = await mkdtemp(path.join(os.tmpdir(), 'domios-headless-readback-'));
  try {
    const names = [local.names.archive, local.names.checksum, local.names.manifest];
    for (let index = 0; index < local.paths.length; index += 1) {
      execFileSync('gh', ['release', 'download', tag, '--pattern', names[index],
        '--dir', scratch, '--repo', repository], { env, stdio: 'inherit', timeout: 300000 });
      const downloaded = await readFile(path.join(scratch, names[index]));
      if (!downloaded.equals(local.bytes[index])) fail('uploaded headless asset differs on release readback');
    }
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
  return local.manifest;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [action, tag, platform, directory, sourceSha, repository] = process.argv.slice(2);
  const options = { tag, platform, directory, sourceSha, repository };
  (action === 'verify' ? verifyHeadlessAssets(options)
    : action === 'publish' ? publishHeadlessAssets(options)
      : Promise.reject(new Error('unknown headless asset action')))
    .then(manifest => process.stdout.write(JSON.stringify(manifest.manifest ?? manifest) + '\n'))
    .catch(error => { process.stderr.write(`headless asset refusal: ${error.message}\n`); process.exitCode = 1; });
}
