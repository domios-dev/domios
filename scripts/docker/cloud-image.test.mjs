import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign as edSign } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  buildHeadlessFeedManifest,
  signHeadlessFeed,
  verifyPinnedReleasePubKey,
  verifySignedHeadlessEnvelope,
} from './sign-headless-feed.mjs';
import {
  computeCompressedBytes,
  formatSummaryMarkdown,
  parseGhcrRepo,
  verifyCloudImage,
  verifyManifestContract,
} from './verify-cloud-image.mjs';

function makeTestMinisignPair(keyIdHex = '0102030405060708') {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const spki = publicKey.export({ type: 'spki', format: 'der' });
  const rawPub = spki.subarray(spki.length - 32);
  const keyId = Buffer.from(keyIdHex, 'hex');
  const pubPayload = Buffer.concat([Buffer.from('Ed'), keyId, rawPub]).toString('base64');
  const pubText = `untrusted comment: test minisign public key\n${pubPayload}\n`;
  return {
    keyId,
    privateKey,
    pubBytes: Buffer.from(pubText, 'utf8'),
    pubBase64: Buffer.from(pubText, 'utf8').toString('base64'),
  };
}

function minisignTestBytes(bytes, { keyId, privateKey }, comment = 'version:1.24.0') {
  const digest = createHash('blake2b512').update(bytes).digest();
  const sig = edSign(null, digest, privateKey);
  const packet = Buffer.concat([Buffer.from('ED'), keyId, sig]).toString('base64');
  const globalSig = edSign(null, Buffer.concat([sig, Buffer.from(comment, 'utf8')]), privateKey).toString('base64');
  const text = `untrusted comment: test signature\n${packet}\ntrusted comment: ${comment}\n${globalSig}\n`;
  return Buffer.from(text, 'utf8').toString('base64');
}

test('packaging/docker/release.pub matches src-tauri/tauri.conf.json updater pubkey', () => {
  const tauriConfText = readFileSync('src-tauri/tauri.conf.json', 'utf8');
  const releasePubBytes = readFileSync('packaging/docker/release.pub');
  assert.doesNotThrow(() => verifyPinnedReleasePubKey({ tauriConfText, releasePubBytes }));
  assert.throws(
    () => verifyPinnedReleasePubKey({ tauriConfText, releasePubBytes: Buffer.from('tampered') }),
    /does not match/,
  );
});

test('signHeadlessFeed builds, signs and verifies a schema-1 envelope matching verify.py', async () => {
  const pair = makeTestMinisignPair();
  const root = mkdtempSync(path.join(tmpdir(), 'domios-feed-test-'));
  try {
    const archivePath = path.join(root, 'domios-headless-v1.24.0-linux-x86_64.tar.gz');
    writeFileSync(archivePath, Buffer.from('fake-release-archive-bytes'));
    const repoRoot = path.join(root, 'repo');
    writeFileSync(
      path.join(root, 'stub'),
      'ok',
    );
    const tauriDir = path.join(repoRoot, 'src-tauri');
    const dockerDir = path.join(repoRoot, 'packaging', 'docker');
    const outDir = path.join(root, 'out');
    await Promise.all([
      import('node:fs/promises').then(fs => fs.mkdir(tauriDir, { recursive: true })),
      import('node:fs/promises').then(fs => fs.mkdir(dockerDir, { recursive: true })),
      import('node:fs/promises').then(fs => fs.mkdir(outDir, { recursive: true })),
    ]);
    writeFileSync(path.join(tauriDir, 'tauri.conf.json'), JSON.stringify({ plugins: { updater: { pubkey: pair.pubBase64 } } }));
    writeFileSync(path.join(dockerDir, 'release.pub'), pair.pubBytes);

    const result = await signHeadlessFeed({
      archivePath,
      tag: 'v1.24.0',
      platform: 'linux-x86_64',
      repository: 'domios-dev/domios',
      outputDir: outDir,
      repoRoot,
      signer: (manifestPath, version) => {
        const sigFile = `${manifestPath}.sig`;
        writeFileSync(sigFile, minisignTestBytes(readFileSync(manifestPath), pair, `version:${version}`));
        return sigFile;
      },
    });
    assert.equal(result.feedName, 'domios-headless-linux-x86_64.json');
    const envelope = JSON.parse(readFileSync(result.feedPath, 'utf8'));
    assert.deepEqual(Object.keys(envelope).sort(), ['manifest', 'signature']);
    const verified = verifySignedHeadlessEnvelope({
      envelope,
      expectedManifest: result.manifest,
      publicKeyBytes: pair.pubBytes,
    });
    assert.equal(verified.schema, 1);
    assert.equal(verified.version, '1.24.0');
    assert.equal(verified.allow_downgrade, false);

    // Tampering with manifest or using a foreign key must be refused.
    const otherId = makeTestMinisignPair('1112131415161718');
    assert.throws(
      () => verifySignedHeadlessEnvelope({ envelope, expectedManifest: result.manifest, publicKeyBytes: otherId.pubBytes }),
      /key ID does not match/,
    );
    const sameIdWrongKey = makeTestMinisignPair('0102030405060708');
    assert.throws(
      () => verifySignedHeadlessEnvelope({ envelope, expectedManifest: result.manifest, publicKeyBytes: sameIdWrongKey.pubBytes }),
      /payload signature verification failed/,
    );
    const tampered = { ...envelope, manifest: Buffer.from(JSON.stringify({ ...result.manifest, size: 999 })).toString('base64') };
    assert.throws(
      () => verifySignedHeadlessEnvelope({ envelope: tampered, expectedManifest: result.manifest, publicKeyBytes: pair.pubBytes }),
      /differs from expected/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('buildHeadlessFeedManifest refuses non-stable versions, wrong filenames, and oversized archives', () => {
  const sha = 'a'.repeat(64);
  assert.throws(
    () => buildHeadlessFeedManifest({ platform: 'linux-x86_64', version: '1.24.0-rc1', archiveName: 'domios-headless-v1.24.0-rc1-linux-x86_64.tar.gz', repository: 'domios-dev/domios', tag: 'v1.24.0-rc1', sha256: sha, size: 10 }),
    /Stable/,
  );
  assert.throws(
    () => buildHeadlessFeedManifest({ platform: 'linux-x86_64', version: '1.24.0', archiveName: 'wrong.tar.gz', repository: 'domios-dev/domios', tag: 'v1.24.0', sha256: sha, size: 10 }),
    /does not match/,
  );
  assert.throws(
    () => buildHeadlessFeedManifest({ platform: 'linux-x86_64', version: '1.24.0', archiveName: 'domios-headless-v1.24.0-linux-x86_64.tar.gz', repository: 'domios-dev/domios', tag: 'v1.24.0', sha256: sha, size: 1024 ** 3 + 1 }),
    /byte size/,
  );
});

test('verifyCloudImage checks anonymous pull, tag digest equality, annotations, and compressed size budget', async () => {
  const digest = 'sha256:' + 'b'.repeat(64);
  const sha = 'c'.repeat(40);
  const manifest = {
    config: { size: 4096 },
    layers: [{ size: 500 * 1024 * 1024 }, { size: 600 * 1024 * 1024 }],
    annotations: {
      'org.opencontainers.image.revision': sha,
      'org.domios.environment': 'release',
    },
  };
  assert.equal(parseGhcrRepo('ghcr.io/domios-dev/domios-cloud'), 'domios-dev/domios-cloud');
  assert.equal(computeCompressedBytes(manifest), 4096 + 1100 * 1024 * 1024);

  const calls = [];
  const fakeFetch = async (url, options = {}) => {
    calls.push({ url, auth: options.headers?.Authorization ?? null });
    if (url.startsWith('https://ghcr.io/token?')) {
      return { ok: true, status: 200, json: async () => ({ token: 'anon-jwt' }) };
    }
    return {
      ok: true,
      status: 200,
      headers: new Map([['docker-content-digest', digest]]),
      json: async () => manifest,
    };
  };

  const result = await verifyCloudImage({
    repo: 'ghcr.io/domios-dev/domios-cloud',
    digest,
    expectedSha: sha,
    expectedEnv: 'release',
    tags: ['1.24.0', 'latest'],
    maxCompressedMiB: 1536,
    fetchImpl: fakeFetch,
  });
  assert.equal(result.anonymousPull, 'ok');
  assert.equal(result.imageRef, `ghcr.io/domios-dev/domios-cloud@${digest}`);
  assert.equal(calls[0].auth, null);
  assert.match(formatSummaryMarkdown(result), /ghcr\.io\/domios-dev\/domios-cloud@sha256:b{64}/);

  // Over-budget image must be refused.
  assert.throws(
    () => verifyManifestContract({ manifest, digest, headerDigest: digest, expectedSha: sha, expectedEnv: 'release', maxCompressedMiB: 1000 }),
    /exceeds budget/,
  );
  // Wrong revision or environment annotation must be refused.
  assert.throws(
    () => verifyManifestContract({ manifest, digest, headerDigest: digest, expectedSha: 'd'.repeat(40), expectedEnv: 'release', maxCompressedMiB: 1536 }),
    /revision annotation/,
  );
  assert.throws(
    () => verifyManifestContract({ manifest, digest, headerDigest: digest, expectedSha: sha, expectedEnv: 'qa', maxCompressedMiB: 1536 }),
    /environment annotation/,
  );
  // Denied anonymous pull must be refused.
  await assert.rejects(
    verifyCloudImage({
      repo: 'ghcr.io/domios-dev/domios-cloud',
      digest,
      expectedSha: sha,
      expectedEnv: 'release',
      tags: ['latest'],
      maxCompressedMiB: 1536,
      fetchImpl: async url => (url.startsWith('https://ghcr.io/token?')
        ? { ok: true, status: 200, json: async () => ({ token: 'anon-jwt' }) }
        : { ok: false, status: 401 }),
    }),
    /Anonymous GHCR pull refused/,
  );
  // Tag resolving to a different digest must be refused.
  await assert.rejects(
    verifyCloudImage({
      repo: 'ghcr.io/domios-dev/domios-cloud',
      digest,
      expectedSha: sha,
      expectedEnv: 'release',
      tags: ['latest'],
      maxCompressedMiB: 1536,
      fetchImpl: async url => {
        if (url.startsWith('https://ghcr.io/token?')) return { ok: true, status: 200, json: async () => ({ token: 'anon-jwt' }) };
        const returned = url.endsWith('/latest') ? 'sha256:' + 'f'.repeat(64) : digest;
        return { ok: true, status: 200, headers: new Map([['docker-content-digest', returned]]), json: async () => manifest };
      },
    }),
    /resolved to/,
  );
});
