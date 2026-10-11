import { mkdtemp, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { prepare } from './prepare.mjs';
import { verifySignature } from '../../scripts/frontend-release/sign.mjs';
import { checksum } from '../../scripts/frontend-release/manifest.mjs';

test('three signed DEV bundles bind compatible, incompatible and broken identities', async () => {
  const output = await mkdtemp(path.join(os.tmpdir(), 'frontend-swap-'));
  try {
    await prepare(new URL('../../scripts/frontend-release/fixtures/dist/', import.meta.url).pathname, output, 'http://127.0.0.1:8769');
    const publicKey = await readFile(path.join(output, 'frontend-test.pub'), 'utf8');
    for (const [name, minimum] of [['good', 2], ['incompatible', 3], ['broken', 2], ['older', 2]]) {
      const bytes = await readFile(path.join(output, name, 'frontend-manifest.json'));
      const signature = await readFile(path.join(output, name, 'frontend-manifest.json.minisig'), 'utf8');
      const manifest = JSON.parse(bytes);
      assert.equal(verifySignature(bytes, signature, publicKey), true);
      assert.equal(manifest.minApiVersion, minimum);
      assert.equal(manifest.maxApiVersion, minimum);
      assert.equal(checksum(await readFile(path.join(output, name, `${manifest.releaseId}.tar.gz`))), manifest.sha256);
      assert.equal(manifest.url.startsWith(`http://127.0.0.1:8769/${name}/`), true);
    }
    assert.match(await readFile(path.join(output, 'broken/dist/index.html'), 'utf8'), /throw new Error/);
    const contract = JSON.parse(await readFile(new URL('../../frontend-release-contract.json', import.meta.url), 'utf8'));
    assert.equal(JSON.parse(await readFile(path.join(output, 'older', 'frontend-manifest.json'))).version, contract.version);
    assert.notEqual(JSON.parse(await readFile(path.join(output, 'good', 'frontend-manifest.json'))).version, contract.version);
  } finally { await rm(output, { recursive: true, force: true }); }
});

test('fixture preparation rejects non-DEV origins', async () => {
  await assert.rejects(prepare('unused', 'unused-output', 'http://example.test'), /loopback/);
});
