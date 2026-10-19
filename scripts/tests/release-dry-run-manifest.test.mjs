import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { aggregate, capture, PLATFORMS } from '../release-dry-run-manifest.mjs';
import { HEADLESS_TARGETS } from '../headless-release-package.mjs';

const tag = 'v9.9.9';
const sourceSha = 'a'.repeat(40);

test('local dry-run uses original signed bytes across all six platforms and refuses incomplete/foreign/rebuilt evidence', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'local-updater-fixture-'));
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const keyId = Buffer.from('0123456789abcdef', 'hex');
  const pubkey = Buffer.from('untrusted comment: fixture key\n' +
    Buffer.concat([Buffer.from('Ed'), keyId, publicKey.export({ format: 'der', type: 'spki' }).subarray(12)]).toString('base64') + '\n').toString('base64');
  const signature = bytes => Buffer.from('untrusted comment: fixture signature\n' +
    Buffer.concat([Buffer.from('ED'), keyId, sign(null, createHash('blake2b512').update(bytes).digest(), privateKey)]).toString('base64') +
    '\ntrusted comment: fixture\nAAAA\n').toString('base64');
  const inputDirectory = path.join(directory, 'input');
  const outputDirectory = path.join(directory, 'output');
  try {
    for (const platform of PLATFORMS) {
      const payload = path.join(directory, 'payload', platform);
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
      await writeFile(path.join(payload, `domios-headless-${tag}-${platform}.tar.gz.manifest.json`),
        JSON.stringify({ schema_version: 1, variant: 'headless', version: '9.9.9', target: HEADLESS_TARGETS[platform].target,
          tag, platform, source_sha: sourceSha }));
      await capture({ tag, platform, sourceSha, artifactDirectory: payload, headlessDirectory: payload,
        outputDirectory: path.join(inputDirectory, platform), pubkey });
    }
    const args = { tag, sourceSha, inputDirectory, outputDirectory, pubkey };
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
    row.sha256 = createHash('sha256').update('rebuilt').digest('hex');
    await writeFile(receiptPath, JSON.stringify(changedReceipt));
    await assert.rejects(aggregate(args), /Command failed/);
    await writeFile(archive, original);
    await writeFile(receiptPath, JSON.stringify(receipt));
    await rm(path.join(inputDirectory, 'windows-aarch64'), { recursive: true });
    await assert.rejects(aggregate(args), /ENOENT/);
    assert.equal((await readFile(path.join(outputDirectory, 'latest.json'), 'utf8')), JSON.stringify(main, null, 2) + '\n');
  } finally { await rm(directory, { recursive: true, force: true }); }
});
