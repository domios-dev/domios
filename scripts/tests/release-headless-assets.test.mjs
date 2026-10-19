import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { headlessAssetNames } from '../headless-release-package.mjs';
import { verifyHeadlessAssets } from '../release-headless-assets.mjs';

test('headless asset readback requires all three matching identity-bound files', async t => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'headless-asset-fixture-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const tag = 'v1.14.9';
  const platform = 'linux-x86_64';
  const sourceSha = 'a'.repeat(40);
  const names = headlessAssetNames(tag, platform);
  const archive = Buffer.from('synthetic archive bytes only');
  const archiveSha256 = createHash('sha256').update(archive).digest('hex');
  const manifest = { schema_version: 1, variant: 'headless', platform,
    target: 'x86_64-unknown-linux-gnu', tag, version: '1.14.9', source_sha: sourceSha,
    archive: names.archive, checksum: names.checksum, archive_sha256: archiveSha256 };
  await writeFile(path.join(directory, names.archive), archive);
  await writeFile(path.join(directory, names.checksum), `${archiveSha256}  ${names.archive}\n`);
  await writeFile(path.join(directory, names.manifest), JSON.stringify(manifest));
  const inputs = { tag, platform, directory, sourceSha };
  assert.equal((await verifyHeadlessAssets(inputs)).manifest.archive_sha256, archiveSha256);
  await assert.rejects(verifyHeadlessAssets({ ...inputs, sourceSha: 'b'.repeat(40) }));
  await writeFile(path.join(directory, names.archive), 'changed archive bytes');
  await assert.rejects(verifyHeadlessAssets(inputs));
  await rm(path.join(directory, names.checksum));
  await assert.rejects(verifyHeadlessAssets(inputs));
});
