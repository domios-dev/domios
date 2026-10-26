import assert from 'node:assert/strict';
import { copyFileSync, readFileSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { parse } from 'yaml';
import { fetchArchive, fetchRunArtifact, install, publish, sha256 } from '../release-frontend-dist.mjs';

const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));

async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), 'frontend-dist-'));
  const dist = path.join(root, 'dist');
  await mkdir(path.join(dist, 'assets'), { recursive: true });
  await writeFile(path.join(dist, 'index.html'), '<html>fixture</html>');
  await writeFile(path.join(dist, 'assets', 'app.js'), 'console.log(1)');
  return { root, dist };
}

test('the run publishes one bundle and every job installs exactly those bytes', async () => {
  const { root, dist } = await fixture();
  try {
    const archive = path.join(root, 'frontend-dist.tar');
    const stored = [];
    const digest = await publish(dist, archive, '41', { put: (...args) => stored.push(args) });
    assert.equal(digest, await sha256(archive));
    assert.deepEqual(stored[0].slice(0, 3), ['put', '41/frontend-dist/frontend-dist.tar', archive]);
    const target = path.join(root, 'platform', 'dist');
    await mkdir(target, { recursive: true });
    await writeFile(path.join(target, 'stale.js'), 'old');
    await install(archive, digest, target);
    assert.equal(await readFile(path.join(target, 'index.html'), 'utf8'), '<html>fixture</html>');
    assert.equal(await readFile(path.join(target, 'assets', 'app.js'), 'utf8'), 'console.log(1)');
    await assert.rejects(readFile(path.join(target, 'stale.js')), /ENOENT/);
    await assert.rejects(install(archive, 'f'.repeat(64), target), /differs from the run's f{64}/);
    await assert.rejects(install(archive, '', target), /digest is required/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('a fetched bundle is verified before later steps are told to use it', async () => {
  const { root, dist } = await fixture();
  try {
    const source = path.join(root, 'source.tar');
    const digest = await publish(dist, source, '41', { put: () => {} });
    const env = { GITHUB_ENV: path.join(root, 'github-env') };
    const fetched = path.join(root, 'fetched.tar');
    const get = (verb, key, destination) => { assert.equal(key, '41/frontend-dist/frontend-dist.tar'); copyFileSync(source, destination); };
    await fetchArchive('41', digest, fetched, { get, env });
    assert.equal(await readFile(env.GITHUB_ENV, 'utf8'), `CODETERM_FRONTEND_ARCHIVE=${fetched}\nCODETERM_FRONTEND_SHA256=${digest}\n`);
    await rm(env.GITHUB_ENV);
    await assert.rejects(fetchArchive('41', 'e'.repeat(64), fetched, { get, env }), /differs from the run's e{64}/);
    await assert.rejects(readFile(env.GITHUB_ENV), /ENOENT/);
    await assert.rejects(publish(path.join(root, 'empty'), source, '41', { put: () => {} }), /no index\.html/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('hosted frontend transport reads the producing run artifact and refuses changed bytes', async () => {
  const { root, dist } = await fixture();
  try {
    const source = path.join(root, 'source.tar');
    const digest = await publish(dist, source, '41', { put: () => {} });
    const fetched = path.join(root, 'hosted', 'frontend-dist.tar');
    const env = { GITHUB_ENV: path.join(root, 'github-env'), GITHUB_REPOSITORY: 'owner/repo', GITHUB_RUN_ATTEMPT: '2' };
    await mkdir(path.dirname(fetched), { recursive: true });
    copyFileSync(source, fetched);
    await fetchRunArtifact('41', digest, fetched, { env });
    assert.equal(await sha256(fetched), digest);
    await rm(env.GITHUB_ENV);
    await assert.rejects(fetchRunArtifact('41', 'f'.repeat(64), fetched, { env }), /differs from the run/);
    await assert.rejects(readFile(env.GITHUB_ENV), /ENOENT/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

