import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { parse } from 'yaml';
import { loadNativeMsvcEnvironment } from '../release-arm64-compile.mjs';

const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const hosted = workflow('release-arm64-compile');

test('native MSVC setup preserves quoted batch paths and target arguments', { skip: process.platform !== 'win32' }, async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'arm64 toolchain '));
  try {
    const batch = path.join(directory, 'tool chain.bat');
    await writeFile(batch, '@echo off\r\nset CT_MSVC_FIXTURE=%1\r\n');
    const result = loadNativeMsvcEnvironment(batch, { ComSpec: process.env.ComSpec, SystemRoot: process.env.SystemRoot });
    assert.equal(result.CT_MSVC_FIXTURE, 'arm64');
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('hosted native arm64 compilation has no secrets, write permissions or private cache', () => {
  const job = hosted.jobs.compile;
  assert.match(job['runs-on'], /windows-11-arm/);
  assert.match(job['runs-on'], /ubuntu-24.04-arm/);
  assert.deepEqual(hosted.permissions, { contents: 'read', actions: 'read' });
  assert.doesNotMatch(JSON.stringify(hosted), /secrets\.|secrets:|self-hosted|tailscale/);
  assert.equal(job.env.CODETERM_USE_SCCACHE, '0');
  assert.equal(job.env.CARGO_BUILD_JOBS, '2');
  assert.equal(job.env.RUSTUP_TOOLCHAIN, '1.99.1-${{ inputs.target }}');
  assert.ok(job['timeout-minutes'] > 0);
  assert.equal(job.steps.find(step => step.uses?.startsWith('actions/checkout')).with.ref, '${{ inputs.source_ref }}');
  const script = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
  assert.match(script, /createUpdaterArtifacts.*false/);
  assert.match(script, /tauri bundle --no-binary-patching/);
  assert.match(script, /prepare_release_bundle_inputs bundle/);

});

test('arm64 graph requires hosted handoff then local signing, verification and existing receipts', () => {
  const release = workflow('release');
  for (const [name, target] of [['linux', 'aarch64-unknown-linux-gnu'], ['windows', 'aarch64-pc-windows-msvc']]) {
    const arm = workflow(`release-${name}-arm64`);
    assert.equal(release.jobs[`${name}-arm64`].uses, `./.github/workflows/release-${name}-arm64.yml`);
    assert.equal(arm.jobs.compile.uses, './.github/workflows/release-arm64-compile.yml');
    assert.equal(arm.jobs.compile.with.target, target);
    assert.equal(arm.jobs.compile.secrets, undefined);
    const final = arm.jobs.build;
    assert.equal(final.needs, 'compile');
    assert.ok(final['runs-on'].includes('self-hosted'));
    assert.equal(final.env.AWS_ACCESS_KEY_ID, '${{ secrets.SCCACHE_AWS_ACCESS_KEY_ID }}');
    assert.equal(final.env.AWS_SECRET_ACCESS_KEY, '${{ secrets.SCCACHE_AWS_SECRET_ACCESS_KEY }}');
    const commands = final.steps.map(step => step.run ?? '').join('\n');
    assert.match(commands, /needs\.compile\.outputs\.sha256/);
    assert.match(commands, /signer sign/);
    assert.match(commands, /release-arm64-handoff\.mjs verify/);
    assert.match(commands, /release-bundle-smoke\.mjs/);
    assert.match(commands, /release-dry-run-manifest\.mjs capture/);
    assert.doesNotMatch(commands, /cargo build|tauri build|release-bundle(?:\s|$)|release-inputs/);
    assert.ok(final.steps.some(step => step.name === 'Retain hosted handoff and final stage proof'));
  }
  assert.match(workflow('release-windows').jobs.build.if, /inputs.arch == 'x64'/);
});
