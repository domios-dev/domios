import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { fullComponentManifest, headlessComponentManifest, verifyFinalPair } from '../release-stage.mjs';

const sourceSha = 'a'.repeat(40);
const serverSha256 = 'b'.repeat(64);
const clientSha256 = 'c'.repeat(64);

for (const target of [
  'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc',
  'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
  'x86_64-apple-darwin', 'aarch64-apple-darwin',
]) {
  test(`${target} full and headless manifests bind one final server digest`, () => {
    const mac = target.endsWith('-apple-darwin');
    const full = fullComponentManifest({ target, sourceSha, serverSha256, clientSha256,
      teamId: mac ? 'JMMWQ9LWF7' : undefined });
    const headless = headlessComponentManifest(full, target);
    assert.equal(full.schema, 2);
    assert.equal(headless.schema, 2);
    assert.equal(full.server_build, sourceSha);
    assert.equal(headless.server_build, sourceSha);
    assert.equal(full.server_sha256, serverSha256);
    assert.equal(headless.server_sha256, serverSha256);
    assert.equal(headless.client_identity.kind, 'none');
    assert.equal(headless.server_executable,
      `domios-server/domios-server${target.includes('windows') ? '.exe' : ''}`);
    if (mac) {
      assert.equal(full.server_executable, '../MacOS/domios-server');
      assert.equal(full.client_identity.kind, 'macos_designated_requirement');
      assert.match(full.client_identity.requirement, /certificate leaf\[subject\.OU\] = "JMMWQ9LWF7"$/);
    } else {
      assert.equal(full.client_identity.kind, 'sha256');
      assert.equal(full.client_identity.sha256, clientSha256);
    }
  });
}

test('full manifests reject absent final client identity', () => {
  assert.throws(() => fullComponentManifest({ target: 'x86_64-pc-windows-msvc', sourceSha, serverSha256 }));
  assert.throws(() => fullComponentManifest({ target: 'aarch64-apple-darwin', sourceSha, serverSha256 }));
});

test('headless identity cannot silently reuse a different final server', () => {
  const full = fullComponentManifest({ target: 'x86_64-unknown-linux-gnu', sourceSha,
    serverSha256, clientSha256 });
  const headless = headlessComponentManifest(full, 'x86_64-unknown-linux-gnu');
  assert.equal(headless.server_sha256, full.server_sha256);
  assert.notDeepEqual(headless.client_identity, full.client_identity);
});

test('full package readback rejects changed shared server, CLI, client and manifest bytes', () => {
  const server = Buffer.from('fixture final server');
  const cli = Buffer.from('fixture final CLI');
  const client = Buffer.from('fixture final client');
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  const target = 'x86_64-unknown-linux-gnu';
  const manifest = fullComponentManifest({ target, sourceSha, serverSha256: hash(server),
    clientSha256: hash(client) });
  const manifestBytes = Buffer.from(JSON.stringify(manifest));
  const inputs = { target, sourceSha, manifest, stagedServer: server, packagedServer: server,
    stagedCli: cli, packagedCli: cli, packagedClient: client,
    stagedManifest: manifestBytes, packagedManifest: manifestBytes };
  assert.deepEqual(verifyFinalPair(inputs), manifest);
  for (const changed of [
    { packagedServer: Buffer.from('changed server') },
    { packagedCli: Buffer.from('changed CLI') },
    { packagedClient: Buffer.from('changed client') },
    { packagedManifest: Buffer.from('{}') },
  ]) assert.throws(() => verifyFinalPair({ ...inputs, ...changed }));
});
