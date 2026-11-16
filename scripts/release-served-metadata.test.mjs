import assert from 'node:assert/strict';
import { generateKeyPairSync, sign } from 'node:crypto';
import test from 'node:test';
import { verifyServedMetadata } from './release-served-metadata.mjs';
import { authorizeClosure } from './check-release-closure.mjs';
import { PLATFORMS } from './release-dry-run-manifest.mjs';

const tag = 'v1.20.1';
const sourceSha = 'a'.repeat(40);
const runId = '123';
const repository = 'domios-dev/domios';
const { privateKey, publicKey } = generateKeyPairSync('ed25519');
const keyId = Buffer.alloc(8, 1);
const signatureBytes = Buffer.alloc(64, 2);
const comment = 'version:1.20.1';
const signature = Buffer.from(['untrusted comment: fixture',
  Buffer.concat([Buffer.from('ED'), keyId, signatureBytes]).toString('base64'),
  `trusted comment: ${comment}`,
  sign(null, Buffer.concat([signatureBytes, Buffer.from(comment)]), privateKey).toString('base64')].join('\n')).toString('base64');

function fixture() {
  const evidence = PLATFORMS.map(platform => ({ platform, tag, source_sha: sourceSha, files: [
    { name: `${platform}.bin`, size: 42, sha256: 'b'.repeat(64) },
    { name: `updater-${platform}.json`, size: 25, sha256: 'c'.repeat(64) },
  ] }));
  for (const row of evidence.filter(row => row.platform.startsWith('linux-'))) {
    row.files.push({ name: `${row.platform}.deb`, size: 52, sha256: 'd'.repeat(64) });
  }
  const entry = file => ({ url: `https://github.com/${repository}/releases/download/${tag}/${file.name}`,
    signature, sha256: file.sha256, size: file.size });
  const manifests = Object.fromEntries(['domios-latest.json', 'domios-latest-deb.json'].map(name => [name,
    { version: '1.20.1', platforms: Object.fromEntries(evidence.filter(row => !name.includes('-deb.') || row.platform.startsWith('linux-'))
      .map(row => [row.platform, entry(row.files[name.includes('-deb.') ? 2 : 0])])) }]));
  return { schema: 1, mode: 'local_dry_run', tag, source_sha: sourceSha, run_id: runId, platforms: PLATFORMS, evidence, manifests };
}

function options(receipt = fixture(), changed = {}) {
  const requests = [];
  const files = receipt.evidence.flatMap(row => row.files);
  return { receipt, tag, sourceSha, runId, repository, key: { keyId, publicKey }, requests,
    request: async (url, init) => {
      requests.push([url, init.method ?? 'GET']);
      const name = new URL(url).pathname.split('/').at(-1);
      if (name.startsWith('domios-latest')) {
        assert.equal(init.method, undefined);
        return { status: 200, json: async () => structuredClone(receipt.manifests[name]) };
      }
      assert.equal(init.method, 'HEAD', 'payloads must never be downloaded');
      return { status: 200, headers: new Headers({ 'content-length': String(files.find(file => file.name === name).size) }), ...changed };
    } };
}

test('served verification GETs only manifests and HEADs all qualified assets, authorizing closure', async () => {
  const input = options();
  const records = await verifyServedMetadata(input);
  assert.equal(input.requests.filter(([, method]) => method === 'GET').length, 2);
  assert.equal(input.requests.filter(([, method]) => method === 'HEAD').length, 14);
  assert.deepEqual(records[0].qualification, { source_sha: sourceSha, run_id: runId });
  assert.doesNotThrow(() => authorizeClosure(...records, '1.20.1'));
  assert.throws(() => authorizeClosure(records[0], { ...records[1], qualification: { source_sha: sourceSha, run_id: '124' } }, '1.20.1'));
});

test('mirror verification checks mirror payload URLs without expecting private updater fragments', async () => {
  const input = options();
  input.repository = 'rollacode/codeterm-releases';
  await verifyServedMetadata(input);
  assert.equal(input.requests.filter(([, method]) => method === 'HEAD').length, 8);
  assert.ok(input.requests.every(([url]) => url.includes('/rollacode/codeterm-releases/')));
});

test('wrong HTTP status, missing content length and wrong size refuse closure evidence', async () => {
  for (const changed of [{ status: 404 }, { status: 206 }, { headers: new Headers() },
    { headers: new Headers({ 'content-length': '43' }) }]) {
    await assert.rejects(verifyServedMetadata(options(fixture(), changed)), /status\/size mismatch/);
  }
});

test('foreign qualification metadata refuses before requests', async () => {
  for (const changed of [{ source_sha: 'b'.repeat(40) }, { run_id: '124' }, { tag: 'v1.20.0' },
    { evidence: [] }, { platforms: PLATFORMS.slice(1) }]) {
    const input = options({ ...fixture(), ...changed });
    await assert.rejects(verifyServedMetadata(input), /Foreign/);
    assert.equal(input.requests.length, 0);
  }
});

test('changed served signature or digest metadata refuses without payload reads', async () => {
  const input = options();
  input.request = async () => {
    const manifest = structuredClone(input.receipt.manifests['domios-latest.json']);
    manifest.platforms[PLATFORMS[0]].signature += 'altered';
    return { status: 200, json: async () => manifest };
  };
  await assert.rejects(verifyServedMetadata(input), /differs from qualification/);
  for (const changed of [{ sha256: 'e'.repeat(64) }, { url: 'https://foreign.example/asset' }, { size: 43 }]) {
    const receipt = fixture();
    Object.assign(receipt.manifests['domios-latest.json'].platforms[PLATFORMS[0]], changed);
    await assert.rejects(verifyServedMetadata(options(receipt)), /differs from qualified digest or URL/);
  }
});

test('trusted version comment signatures are still cryptographically verified', async () => {
  const receipt = fixture();
  const text = Buffer.from(signature, 'base64').toString('utf8').replace('version:1.20.1', 'version:1.20.2');
  receipt.manifests['domios-latest.json'].platforms[PLATFORMS[0]].signature = Buffer.from(text).toString('base64');
  await assert.rejects(verifyServedMetadata(options(receipt)), /trusted comment signature does not verify/);
});

test('served and issue closure evidence retain the exact owner test waiver', async () => {
  const input = options();
  input.waiver = { run_id: runId, jobs: ['Fast checks / ts unit (1/4)'] };
  const records = await verifyServedMetadata(input);
  assert.deepEqual(records[0].qualification.test_waiver, input.waiver);
  assert.doesNotThrow(() => authorizeClosure(...records, '1.20.1'));
  const stripped = structuredClone(records[1]);
  delete stripped.qualification.test_waiver;
  assert.throws(() => authorizeClosure(records[0], stripped, '1.20.1'));
  const { closeVerifiedIssues } = await import('./check-release-closure.mjs');
  const closed = [];
  await closeVerifiedIssues([{ number: 42, comment: 'Released in v1.20.1\nVerified publication.' }],
    ...records, '1.20.1', async action => closed.push(action));
  assert.ok(closed[0].comment.includes('qualification run 123'));
  assert.ok(closed[0].comment.includes(input.waiver.jobs[0]));
  input.waiver = { ...input.waiver, run_id: '124' };
  await assert.rejects(verifyServedMetadata(input), /Foreign served metadata waiver/);
});
