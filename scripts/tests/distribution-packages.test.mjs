import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { prepare } from '../../distribution/prepare.mjs';

async function fixture(t) {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'domios-distribution-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const sourceSha = 'a'.repeat(40);
  const version = '9.8.7';
  const inputDirectory = path.join(directory, 'retained');
  const outputDirectory = path.join(directory, 'output');
  const receiptFile = path.join(directory, 'manifest-local-verification.json');
  const receipt = { schema: 1, mode: 'local_dry_run', tag: `v${version}`, source_sha: sourceSha, evidence: [] };
  const files = [
    ['windows-x86_64', `Domios_${version}_x64-setup.exe`],
    ['windows-aarch64', `Domios_${version}_arm64-setup.exe`],
    ['darwin-x86_64', `Domios_${version}_x64.dmg`],
    ['darwin-aarch64', `Domios_${version}_aarch64.dmg`],
  ];
  for (const [platform, name] of files) {
    const bytes = Buffer.from(`synthetic desktop fixture: ${platform}`);
    await mkdir(path.join(inputDirectory, platform), { recursive: true });
    await writeFile(path.join(inputDirectory, platform, name), bytes);
    receipt.evidence.push({ schema: 1, tag: receipt.tag, source_sha: sourceSha, platform,
      files: [{ name, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }] });
  }
  const save = () => writeFile(receiptFile, JSON.stringify(receipt));
  await save();
  return { options: { sourceSha, receiptFile, inputDirectory, outputDirectory }, receipt, save, directory };
}

test('package preparation pins four desktop assets and links the actual bundled CLI', async t => {
  const { options } = await fixture(t);
  const result = await prepare(options);
  assert.equal(result.assets.length, 4);
  const scoop = JSON.parse(await readFile(path.join(options.outputDirectory, 'scoop/bucket/domios.json'), 'utf8'));
  const cask = await readFile(path.join(options.outputDirectory, 'homebrew/Casks/domios.rb'), 'utf8');
  assert.equal(scoop.version, '9.8.7');
  assert.deepEqual(Object.keys(scoop.architecture).sort(), ['64bit', 'arm64']);
  assert.deepEqual(scoop.bin, ['cli\\domios.exe', 'cli\\codeterm.exe']);
  assert.deepEqual(scoop.shortcuts, [['domios-app.exe', 'Domios']]);
  for (const [index, architecture] of ['64bit', 'arm64'].entries()) {
    assert.equal(scoop.architecture[architecture].url, result.assets[index].url + '#/installer.exe');
    assert.equal(scoop.architecture[architecture].hash, result.assets[index].sha256);
  }
  assert.match(cask, /version "9\.8\.7"/);
  assert.match(cask, /arch arm: "aarch64", intel: "x64"/);
  assert.match(cask, /binary "#\{appdir\}\/Domios\.app\/Contents\/Resources\/cli\/domios"/);
  assert.match(cask, /binary "#\{appdir\}\/Domios\.app\/Contents\/Resources\/cli\/codeterm"/);
  for (const asset of result.assets.slice(2)) assert.ok(cask.includes(`sha256 "${asset.sha256}"`));
  assert.equal(/@[A-Z0-9_]+@/.test(JSON.stringify(scoop) + cask), false);
  assert.equal((JSON.stringify(scoop) + cask).includes('rollacode/codeterm-releases'), false);
  assert.deepEqual(JSON.parse(await readFile(path.join(options.outputDirectory, 'package-inputs.json'), 'utf8')), result);
});

test('package preparation rejects changed desktop bytes before creating output', async t => {
  const { options, receipt, directory } = await fixture(t);
  const platform = receipt.evidence[0];
  await writeFile(path.join(options.inputDirectory, platform.platform, platform.files[0].name), 'changed');
  await assert.rejects(prepare(options), /digest mismatch/);
  assert.equal((await readdir(directory)).includes('output'), false);
});

test('package preparation rejects a foreign source and prerelease tag', async t => {
  const { options, receipt, save } = await fixture(t);
  await assert.rejects(prepare({ ...options, sourceSha: 'b'.repeat(40) }), /pinned source SHA/);
  receipt.tag += '-rc.1';
  await save();
  await assert.rejects(prepare(options), /pinned source SHA/);
});

test('package preparation requires exactly one receipt for every architecture', async t => {
  const { options, receipt, save } = await fixture(t);
  const removed = receipt.evidence.pop();
  await save();
  await assert.rejects(prepare(options), /platform receipt: darwin-aarch64/);
  receipt.evidence.push(removed, removed);
  await save();
  await assert.rejects(prepare(options), /platform receipt: darwin-aarch64/);
});

test('package preparation refuses an updater archive substituted for the desktop DMG', async t => {
  const { options, receipt, save } = await fixture(t);
  receipt.evidence[2].files[0].name = 'Domios_x64.app.tar.gz';
  await save();
  await assert.rejects(prepare(options), /desktop asset receipt: Domios_9.8.7_x64.dmg/);
});

test('package preparation preserves a nonempty review output', async t => {
  const { options } = await fixture(t);
  await mkdir(options.outputDirectory);
  await writeFile(path.join(options.outputDirectory, 'keep.txt'), 'existing review');
  await assert.rejects(prepare(options), /must be empty/);
  assert.deepEqual(await readdir(options.outputDirectory), ['keep.txt']);
  assert.equal(await readFile(path.join(options.outputDirectory, 'keep.txt'), 'utf8'), 'existing review');
});
