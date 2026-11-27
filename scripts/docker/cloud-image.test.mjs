import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign as edSign } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { readHeadlessArchive } from '../headless-release-package.mjs';
import { packageCloudQa, prepareQaCliDigest } from './package-cloud-qa.mjs';
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

test('packageCloudQa strips debug info before packaging, hashes shipped stripped bytes, and retains debug symbols', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'domios-cloud-qa-strip-'));
  const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
  try {
    const target = 'x86_64-unknown-linux-gnu';
    const sourceSha = 'e'.repeat(40);
    const version = '1.24.0';
    const repoRoot = path.join(root, 'repo');
    const logs = path.join(root, 'logs');
    const built = path.join(root, 'built');
    for (const dir of [
      path.join(repoRoot, 'cli', 'src'),
      path.join(repoRoot, 'src-tauri', 'crates', 'domios-server', 'src'),
      path.join(repoRoot, 'dist', 'assets'),
      path.join(repoRoot, 'skills', 'demo'),
      logs,
      built,
    ]) {
      mkdirSync(dir, { recursive: true });
    }
    const cliSrc = path.join(repoRoot, 'cli', 'src', 'main.rs');
    const serverSrc = path.join(repoRoot, 'src-tauri', 'crates', 'domios-server', 'src', 'main.rs');
    writeFileSync(cliSrc, 'fn main() {}');
    writeFileSync(serverSrc, 'fn main() {}');
    writeFileSync(path.join(repoRoot, 'src-tauri', 'tauri.conf.json'), JSON.stringify({ version }));
    writeFileSync(path.join(repoRoot, 'dist', 'index.html'), '<html><script src="/assets/app.js"></script></html>');
    writeFileSync(path.join(repoRoot, 'dist', 'assets', 'app.js'), 'console.log("ok");');
    writeFileSync(path.join(repoRoot, 'skills', 'demo', 'SKILL.md'), '# demo\n');
    writeFileSync(path.join(repoRoot, 'LICENSE'), 'AGPL\n');
    writeFileSync(path.join(repoRoot, 'NOTICE'), 'Domios\n');

    const debugMarker = Buffer.from('::DEBUG_SYMBOLS::');
    const strippedCliBytes = Buffer.from('ELF-CLI-CODE');
    const unstrippedCliBytes = Buffer.concat([strippedCliBytes, debugMarker, Buffer.alloc(2048, 0xdd)]);
    const strippedServerBytes = Buffer.from('ELF-SERVER-CODE');
    const unstrippedServerBytes = Buffer.concat([strippedServerBytes, debugMarker, Buffer.alloc(4096, 0xee)]);
    const rawCliPath = path.join(built, 'codeterm');
    const rawServerPath = path.join(built, 'domios-server');
    writeFileSync(rawCliPath, unstrippedCliBytes);
    writeFileSync(rawServerPath, unstrippedServerBytes);

    const cliLog = path.join(logs, 'cli.jsonl');
    writeFileSync(
      cliLog,
      [
        JSON.stringify({
          reason: 'compiler-artifact',
          target: { name: 'codeterm', kind: ['bin'], src_path: cliSrc },
          profile: { debug_assertions: true, test: false },
          executable: rawCliPath,
        }),
        JSON.stringify({ reason: 'build-finished', success: true }),
      ].join('\n') + '\n',
    );
    writeFileSync(path.join(logs, 'closure.txt'), 'domios-server 0.0.0\n');

    const objcopyCalls = [];
    const fakeObjcopy = (cmd, args) => {
      objcopyCalls.push([cmd, ...args]);
      const input = readFileSync(args[args.length - 2]);
      const output = args[args.length - 1];
      const splitAt = input.indexOf(debugMarker);
      assert.notEqual(splitAt, -1, 'fixture binary must contain synthetic debug section');
      if (args.includes('--only-keep-debug')) {
        writeFileSync(output, input.subarray(splitAt));
      } else if (args.includes('--strip-debug')) {
        writeFileSync(output, input.subarray(0, splitAt));
      } else {
        throw new Error(`Unexpected objcopy args: ${args.join(' ')}`);
      }
    };

    const strippedRoot = path.join(root, 'stripped');
    const debugSymbolsRoot = path.join(root, 'debug-symbols');
    const cliPrepared = await prepareQaCliDigest({
      cliLog,
      target,
      repoRoot,
      strippedRoot,
      debugSymbolsRoot,
      objcopy: 'fake-objcopy',
      run: fakeObjcopy,
    });
    assert.equal(cliPrepared.sha256, sha256(strippedCliBytes));
    assert.notEqual(cliPrepared.sha256, sha256(unstrippedCliBytes));
    assert.equal(cliPrepared.strippedBytes, strippedCliBytes.length);
    assert.equal(cliPrepared.originalBytes, unstrippedCliBytes.length);

    const writeServerLog = cliDigestEnv => {
      writeFileSync(
        path.join(logs, 'server.jsonl'),
        [
          JSON.stringify({
            reason: 'build-script-executed',
            package_id: 'domios-server 0.0.0',
            env: [
              ['BUILD_SHA', sourceSha],
              ['SERVER_CLI_SHA256', cliDigestEnv],
            ],
          }),
          JSON.stringify({
            reason: 'compiler-artifact',
            target: { name: 'domios-server', kind: ['bin'], src_path: serverSrc },
            profile: { debug_assertions: true, test: false },
            features: ['embeddings', 'mem-postgres'],
            executable: rawServerPath,
          }),
          JSON.stringify({ reason: 'build-finished', success: true }),
        ].join('\n') + '\n',
      );
    };

    // Unstripped CLI digest embedded in domios-server must be refused.
    writeServerLog(sha256(unstrippedCliBytes));
    await assert.rejects(
      packageCloudQa({
        target,
        logs,
        repoRoot,
        stageRoot: path.join(root, 'stage-bad'),
        outputDir: path.join(root, 'archives-bad'),
        strippedRoot,
        debugSymbolsRoot,
        sourceSha,
        version,
        objcopy: 'fake-objcopy',
        run: fakeObjcopy,
      }),
      /CLI digest mismatch/,
    );

    // Stripped CLI digest succeeds and packages stripped binaries with matching manifests and retained debug symbols.
    writeServerLog(cliPrepared.sha256);
    const summary = await packageCloudQa({
      target,
      logs,
      repoRoot,
      stageRoot: path.join(root, 'stage-ok'),
      outputDir: path.join(root, 'archives-ok'),
      strippedRoot,
      debugSymbolsRoot,
      sourceSha,
      version,
      objcopy: 'fake-objcopy',
      run: fakeObjcopy,
    });

    assert.equal(summary.server_sha256, sha256(strippedServerBytes));
    assert.notEqual(summary.server_sha256, sha256(unstrippedServerBytes));
    assert.equal(summary.cli_sha256, sha256(strippedCliBytes));
    assert.equal(summary.server_bytes, strippedServerBytes.length);
    assert.equal(summary.server_unstripped_bytes, unstrippedServerBytes.length);
    assert.equal(summary.cli_bytes, strippedCliBytes.length);
    assert.equal(summary.cli_unstripped_bytes, unstrippedCliBytes.length);

    const archiveFiles = readHeadlessArchive(readFileSync(summary.archive));
    assert.deepEqual(archiveFiles.get('domios-server/domios-server').bytes, strippedServerBytes);
    assert.deepEqual(archiveFiles.get('cli/codeterm').bytes, strippedCliBytes);
    assert.deepEqual(archiveFiles.get('cli/domios').bytes, strippedCliBytes);
    const componentManifest = JSON.parse(archiveFiles.get('server-component.json').bytes.toString('utf8'));
    assert.equal(componentManifest.server_sha256, sha256(strippedServerBytes));

    // Separate debug symbol files remain intact for CI artifact upload.
    assert.equal(readFileSync(summary.debug_symbols.server).indexOf(debugMarker), 0);
    assert.equal(readFileSync(summary.debug_symbols.cli).indexOf(debugMarker), 0);
    assert.ok(objcopyCalls.some(call => call.includes('--only-keep-debug')));
    assert.ok(objcopyCalls.some(call => call.includes('--strip-debug')));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
