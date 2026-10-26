import { condition as workflowCondition } from './workflow-condition.mjs';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFileSync, readFileSync, writeFileSync } from 'node:fs';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { parse } from 'yaml';
import { headlessAssetNames } from '../headless-release-package.mjs';
import { checkQualifiedRun, promotionPlan, publishPlatform, REQUIRED_JOBS } from '../release-promote.mjs';

const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const release = workflow('release');
const sha = 'b'.repeat(40);
const branch = 'release/9.9.9';
const qualifiedJobs = () => [
  'prepare release', 'Fast checks / guards', 'Fast checks / clippy (linux)', 'Fast checks / ts unit', 'Fast checks / ts vitest',
  'Fast checks / rust tests (full suite)', 'Rust tests / rust tests (full suite) (full)', 'Rust tests / rust platform (Linux)',
  'Rust tests / rust platform (Windows)', 'macOS arm64 / ', 'Rust tests / rust embeddings (embeddings)',
  'Rust tests / rust test receipts', 'macOS arm64 / build (arm64, aarch64, macos-ci)', 'macOS x64 / build (x86_64, x64, macos-ci)',
  'Linux arm64 / build', 'Linux x64 / build (x86_64-unknown-linux-gnu)', 'Windows x64 / build (x86_64-pc-windows-msvc)',
  'Windows arm64 / build (aarch64-pc-windows-msvc)', 'local dry-run manifest verification / verify consolidated manifests against private CI bytes',
  'release manifest, mirror, and verification',
].map(name => ({ name, conclusion: /rust tests \(full suite\)$|^release manifest/.test(name) ? 'skipped' : 'success' }));
const qualifiedRun = { id: 41, path: '.github/workflows/release.yml', status: 'completed', conclusion: 'success', head_sha: sha, head_branch: branch };



test('the source draft takes every retained distributable and fragment; the mirror takes no fragment', () => {
  const tag = 'v9.9.9';
  const headless = headlessAssetNames(tag, 'darwin-aarch64');
  const receipt = { files: ['Domios_9.9.9_aarch64.dmg', 'Domios_aarch64.app.tar.gz', 'updater-darwin-aarch64.json',
    headless.archive, headless.checksum, headless.manifest].map(name => ({ name })) };
  assert.deepEqual(promotionPlan(tag, 'darwin-aarch64', receipt), {
    source: ['Domios_9.9.9_aarch64.dmg', 'Domios_aarch64.app.tar.gz', 'updater-darwin-aarch64.json'],
    mirror: ['Domios_9.9.9_aarch64.dmg', 'Domios_aarch64.app.tar.gz'],
  });
});

test('promotion uploads the retained bytes, reads each back and refuses a changed asset', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'promote-fixture-'));
  try {
    const tag = 'v9.9.9';
    const names = ['Domios_9.9.9_amd64.AppImage', 'Domios_9.9.9_amd64.deb', 'updater-linux-x86_64.json', 'updater-linux-x86_64-deb.json'];
    const files = [];
    for (const name of names) {
      await writeFile(path.join(directory, name), 'bytes of ' + name);
      files.push({ name, sha256: createHash('sha256').update('bytes of ' + name).digest('hex') });
    }
    const receipt = { files };
    const calls = [];
    const headless = [];
    let tamper = '';
    const run = args => {
      calls.push(args);
      if (args[1] !== 'download') return;
      const name = args[args.indexOf('--pattern') + 1];
      const target = path.join(args[args.indexOf('--dir') + 1], name);
      copyFileSync(path.join(directory, name), target);
      if (name === tamper) writeFileSync(target, 'served something else');
    };
    const options = { tag, sourceSha: sha, platform: 'linux-x86_64', directory, receipt, env: {}, run,
      publishHeadless: options => headless.push(options.repository) };
    assert.deepEqual(await publishPlatform({ ...options, repository: 'rollacode/codeterm-releases' }), names.slice(0, 2));
    assert.deepEqual(calls[0].slice(0, 3), ['release', 'upload', tag]);
    assert.equal(calls[0].filter(arg => arg.includes('updater-')).length, 0);
    assert.equal(calls.filter(args => args[1] === 'download').length, 2);
    assert.deepEqual(await publishPlatform({ ...options, repository: 'rollacode/codeterm' }), names);
    assert.deepEqual(headless, ['rollacode/codeterm-releases', 'rollacode/codeterm']);
    tamper = 'Domios_9.9.9_amd64.deb';
    await assert.rejects(publishPlatform({ ...options, repository: 'rollacode/codeterm' }),
      /rollacode\/codeterm Domios_9\.9\.9_amd64\.deb differs from its linux-x86_64 receipt/);
    assert.equal(headless.length, 2, 'a refused platform never reaches headless publication');
  } finally { await rm(directory, { recursive: true, force: true }); }
});


test('the promotion CLI calls only gh flags every runner build has', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = await readFile(new URL('../release-promote.mjs', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /'--slurp'/);
});
