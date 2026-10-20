import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { aggregate, capture, PLATFORMS, store, storeTimeoutMs } from '../release-dry-run-manifest.mjs';
import { HEADLESS_TARGETS, headlessAssetNames } from '../headless-release-package.mjs';

const tag = 'v9.9.9';
const sourceSha = 'a'.repeat(40);
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const dmgName = platform => `CodeTerm_9.9.9_${platform.endsWith('aarch64') ? 'aarch64' : 'x64'}.dmg`;

async function fixture() {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'local-updater-fixture-'));
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const keyId = Buffer.from('0123456789abcdef', 'hex');
  const pubkey = Buffer.from('untrusted comment: fixture key\n' +
    Buffer.concat([Buffer.from('Ed'), keyId, publicKey.export({ format: 'der', type: 'spki' }).subarray(12)]).toString('base64') + '\n').toString('base64');
  const signature = bytes => Buffer.from('untrusted comment: fixture signature\n' +
    Buffer.concat([Buffer.from('ED'), keyId, sign(null, createHash('blake2b512').update(bytes).digest(), privateKey)]).toString('base64') +
    '\ntrusted comment: fixture\nAAAA\n').toString('base64');
  const payloadDirectory = platform => path.join(directory, 'payload', platform);
  for (const platform of PLATFORMS) {
    const payload = payloadDirectory(platform);
    await mkdir(payload, { recursive: true });
    const names = platform.startsWith('darwin-')
      ? [`CodeTerm_9.9.9_${platform.endsWith('aarch64') ? 'aarch64' : 'x64'}.app.tar.gz`]
      : platform.startsWith('windows-') ? [`CodeTerm_9.9.9_${platform.endsWith('aarch64') ? 'arm64' : 'x64'}-setup.exe`]
        : [`CodeTerm_9.9.9_${platform.endsWith('aarch64') ? 'aarch64' : 'amd64'}.AppImage`,
          `CodeTerm_9.9.9_${platform.endsWith('aarch64') ? 'arm64' : 'amd64'}.deb`];
    for (const name of names) {
      const bytes = Buffer.from('synthetic artifact ' + name);
      await writeFile(path.join(payload, name), bytes);
      await writeFile(path.join(payload, name + '.sig'), signature(bytes));
    }
    if (platform.startsWith('darwin-')) await writeFile(path.join(payload, dmgName(platform)), 'synthetic disk image ' + platform);
    const headless = headlessAssetNames(tag, platform);
    const archive = Buffer.from('synthetic headless archive ' + platform);
    await writeFile(path.join(payload, headless.archive), archive);
    await writeFile(path.join(payload, headless.checksum), `${digest(archive)}  ${headless.archive}\n`);
    await writeFile(path.join(payload, headless.manifest), JSON.stringify({ schema_version: 1, variant: 'headless', platform,
      target: HEADLESS_TARGETS[platform].target, tag, version: '9.9.9', source_sha: sourceSha,
      archive: headless.archive, checksum: headless.checksum, archive_sha256: digest(archive) }));
  }
  const inputDirectory = path.join(directory, 'input');
  const captureOne = platform => capture({ tag, platform, sourceSha, artifactDirectory: payloadDirectory(platform),
    headlessDirectory: payloadDirectory(platform), outputDirectory: path.join(inputDirectory, platform), pubkey });
  return { directory, pubkey, inputDirectory, payloadDirectory, captureOne,
    args: { tag, sourceSha, inputDirectory, outputDirectory: path.join(directory, 'output'), pubkey } };
}

test('local dry-run uses original signed bytes across all six platforms and refuses incomplete/foreign/rebuilt evidence', async () => {
  const { directory, inputDirectory, captureOne, args } = await fixture();
  const { outputDirectory } = args;
  try {
    for (const platform of PLATFORMS) await captureOne(platform);
    const result = await aggregate(args);
    assert.equal(result.mode, 'local_dry_run');
    assert.deepEqual(result.platforms, PLATFORMS);
    const main = JSON.parse(await readFile(path.join(outputDirectory, 'latest.json'), 'utf8'));
    assert.deepEqual(Object.keys(main.platforms).sort(), PLATFORMS);
    const deb = JSON.parse(await readFile(path.join(outputDirectory, 'latest-deb.json'), 'utf8'));
    assert.deepEqual(Object.keys(deb.platforms).sort(), ['linux-aarch64', 'linux-x86_64']);

    const receiptPath = path.join(inputDirectory, 'darwin-aarch64', 'dry-run-evidence.json');
    const receipt = JSON.parse(await readFile(receiptPath, 'utf8'));
    await writeFile(receiptPath, JSON.stringify({ ...receipt, source_sha: 'b'.repeat(40) }));
    await assert.rejects(aggregate(args), /foreign dry-run/);
    await writeFile(receiptPath, JSON.stringify(receipt));
    const archive = path.join(inputDirectory, 'darwin-aarch64', 'CodeTerm_aarch64.app.tar.gz');
    const original = await readFile(archive);
    await writeFile(archive, 'rebuilt');
    await assert.rejects(aggregate(args), /digest mismatch/);
    const changedReceipt = structuredClone(receipt);
    const row = changedReceipt.files.find(file => file.name.endsWith('.app.tar.gz'));
    row.size = 7;
    row.sha256 = digest('rebuilt');
    await writeFile(receiptPath, JSON.stringify(changedReceipt));
    await assert.rejects(aggregate(args), /Command failed/);
    await writeFile(archive, original);
    await writeFile(receiptPath, JSON.stringify(receipt));
    await rm(path.join(inputDirectory, 'windows-aarch64'), { recursive: true });
    await assert.rejects(aggregate(args), /ENOENT/);
    assert.equal((await readFile(path.join(outputDirectory, 'latest.json'), 'utf8')), JSON.stringify(main, null, 2) + '\n');
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('capture retains DMG and headless archive, checksum and metadata byte-identically with receipt rows', async () => {
  const { directory, inputDirectory, payloadDirectory, captureOne } = await fixture();
  try {
    for (const platform of PLATFORMS) {
      const receipt = await captureOne(platform);
      const headless = headlessAssetNames(tag, platform);
      const distributables = [...(platform.startsWith('darwin-') ? [dmgName(platform)] : []),
        headless.archive, headless.checksum, headless.manifest];
      assert.equal(receipt.tag, tag);
      assert.equal(receipt.platform, platform);
      assert.equal(receipt.source_sha, sourceSha);
      for (const name of distributables) {
        const source = await readFile(path.join(payloadDirectory(platform), name));
        const retained = await readFile(path.join(inputDirectory, platform, name));
        assert.ok(retained.equals(source), name);
        assert.deepEqual(receipt.files.find(file => file.name === name), { name, size: source.length, sha256: digest(source) });
      }
      const stored = (await readdir(path.join(inputDirectory, platform))).sort();
      assert.deepEqual(stored, ['dry-run-evidence.json', ...receipt.files.map(file => file.name)].sort());
    }
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('capture refuses a missing DMG or headless checksum', async () => {
  const { directory, payloadDirectory, captureOne } = await fixture();
  try {
    await rm(path.join(payloadDirectory('darwin-x86_64'), dmgName('darwin-x86_64')));
    await assert.rejects(captureOne('darwin-x86_64'), /expected one darwin-x86_64 artifact CodeTerm_9\.9\.9_x64\.dmg; found 0/);
    await rm(path.join(payloadDirectory('linux-aarch64'), headlessAssetNames(tag, 'linux-aarch64').checksum));
    await assert.rejects(captureOne('linux-aarch64'), /ENOENT/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('aggregate refuses a changed DMG or headless byte and a DMG missing from the store', async () => {
  const { directory, inputDirectory, captureOne, args } = await fixture();
  try {
    for (const platform of PLATFORMS) await captureOne(platform);
    const dmg = path.join(inputDirectory, 'darwin-aarch64', dmgName('darwin-aarch64'));
    const dmgBytes = await readFile(dmg);
    await writeFile(dmg, Buffer.concat([dmgBytes.subarray(0, -1), Buffer.from('!')]));
    await assert.rejects(aggregate(args), /digest mismatch: CodeTerm_9\.9\.9_aarch64\.dmg/);
    await writeFile(dmg, dmgBytes);

    const headless = headlessAssetNames(tag, 'windows-x86_64');
    const archive = path.join(inputDirectory, 'windows-x86_64', headless.archive);
    const receiptPath = path.join(inputDirectory, 'windows-x86_64', 'dry-run-evidence.json');
    const receipt = JSON.parse(await readFile(receiptPath, 'utf8'));
    const archiveBytes = await readFile(archive);
    const tampered = Buffer.concat([archiveBytes, Buffer.from('x')]);
    await writeFile(archive, tampered);
    await assert.rejects(aggregate(args), /digest mismatch/);
    const rewritten = structuredClone(receipt);
    Object.assign(rewritten.files.find(file => file.name === headless.archive), { size: tampered.length, sha256: digest(tampered) });
    await writeFile(receiptPath, JSON.stringify(rewritten));
    await assert.rejects(aggregate(args), /headless assets fail/);
    await writeFile(archive, archiveBytes);
    await writeFile(receiptPath, JSON.stringify(receipt));
    await aggregate(args);

    const darwinReceiptPath = path.join(inputDirectory, 'darwin-aarch64', 'dry-run-evidence.json');
    const darwinReceipt = JSON.parse(await readFile(darwinReceiptPath, 'utf8'));
    await rm(dmg);
    await assert.rejects(aggregate(args), /ENOENT/);
    await writeFile(darwinReceiptPath, JSON.stringify({ ...darwinReceipt,
      files: darwinReceipt.files.filter(file => file.name !== dmgName('darwin-aarch64')) }));
    await assert.rejects(aggregate(args), /incomplete platform evidence: darwin-aarch64/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('store sizes its transfer deadline and retries a timed-out transfer once with the elapsed time', () => {
  const hundredMiB = 100 * 1024 * 1024;
  assert.ok(storeTimeoutMs(hundredMiB) > 12 * 60_000);
  assert.ok(storeTimeoutMs(hundredMiB) > storeTimeoutMs(0));
  assert.equal(storeTimeoutMs(0), 20 * 60_000);
  const timedOut = () => Object.assign(new Error('spawnSync bash ETIMEDOUT'), { code: 'ETIMEDOUT' });
  let clock = 0;
  const now = () => clock;
  const calls = [];
  const logs = [];
  const slowOnce = (...args) => {
    calls.push(args);
    clock += 1_500_000;
    if (calls.length === 1) throw timedOut();
    return 'stored';
  };
  assert.equal(store('put', 'k', 'f', { bytes: hundredMiB, run: slowOnce, now, log: line => logs.push(line) }), 'stored');
  assert.equal(calls.length, 2);
  assert.equal(calls[0][3], storeTimeoutMs(hundredMiB));
  assert.equal(logs.length, 1);
  assert.match(logs[0], /put k \(104857600 bytes\) timed out after 1500s; retrying once/);
  const always = () => { throw timedOut(); };
  assert.throws(() => store('put', 'k', 'f', { run: always, now, log: () => {} }), /ETIMEDOUT/);
  let refused = 0;
  const storeError = () => { refused++; throw Object.assign(new Error('curl: (22) 403'), { status: 22 }); };
  assert.throws(() => store('put', 'k', 'f', { run: storeError, now, log: () => {} }), /403/);
  assert.equal(refused, 1);
});
