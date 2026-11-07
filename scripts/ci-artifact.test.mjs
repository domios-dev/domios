import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { bashInterpreter } from './bash-interpreter.mjs';
import { COPY_ORDER_SKEW_MS, materializeFallback } from './ci-artifact-fallback.mjs';

const script = path.join(import.meta.dirname, 'ci-artifact.sh');
const RUN = '42391094820';
const PREFIX = `${RUN}/release-dry-run/darwin-x86_64/`;

// The private store behind a fake curl: objects are files under <root>/<bucket>/<key>, LastModified is their mtime.
const FAKE_CURL = String.raw`
const fs = require('node:fs'), path = require('node:path');
const args = process.argv.slice(2), root = process.env.FAKE_STORE_ROOT;
const url = new URL(args.find(arg => /^https?:\/\//.test(arg)));
const option = name => args.includes(name) ? args[args.indexOf(name) + 1] : undefined;
if (option('--config') === '-') fs.readFileSync(0);
if (url.pathname.endsWith('/minio/health/live')) {
  const probes = path.join(root, '.probe-failures');
  const left = fs.existsSync(probes) ? Number(fs.readFileSync(probes, 'utf8')) : 0;
  if (left > 0) { fs.writeFileSync(probes, String(left - 1)); process.exit(28); }
  process.exit(0);
}
const [, bucket, ...rest] = url.pathname.split('/');
if (url.searchParams.get('list-type') === '2') {
  const prefix = url.searchParams.get('prefix'), base = path.join(root, bucket);
  const walk = dir => fs.existsSync(dir) ? fs.readdirSync(dir, {withFileTypes: true}).flatMap(entry =>
    entry.isDirectory() ? walk(path.join(dir, entry.name)) : [path.join(dir, entry.name)]) : [];
  const keys = walk(base).map(file => path.relative(base, file).split(path.sep).join('/')).filter(key => key.startsWith(prefix)).sort();
  process.stdout.write('<ListBucketResult><IsTruncated>false</IsTruncated>' + keys.map(key =>
    '<Contents><Key>' + key + '</Key><LastModified>' + fs.statSync(path.join(base, key)).mtime.toISOString() + '</LastModified></Contents>').join('') + '</ListBucketResult>');
  process.exit(0);
}
const object = path.join(root, bucket, ...rest);
if (option('--upload-file')) { fs.mkdirSync(path.dirname(object), {recursive: true}); fs.copyFileSync(option('--upload-file'), object); process.exit(0); }
if (!fs.existsSync(object)) process.exit(22);
if (args.includes('--head')) { process.stdout.write('content-length: ' + fs.statSync(object).size + '\r\n'); process.exit(0); }
if (option('--output')) fs.copyFileSync(object, option('--output')); else process.stdout.write(fs.readFileSync(object));
`;

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ci-artifact-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const fake = path.join(root, 'fake-curl.cjs');
  fs.writeFileSync(fake, FAKE_CURL);
  const dirs = Object.fromEntries(['store', 'temp', 'out'].map(name => [name, path.join(root, name)]));
  for (const dir of Object.values(dirs)) fs.mkdirSync(dir);
  const sources = path.join(root, 'sources.tsv');
  fs.writeFileSync(sources, '');
  const posix = file => file.replaceAll('\\', '/');
  // An exported function shadows curl even where the shell prepends its own PATH (Git for Windows).
  const env = { ...process.env, 'BASH_FUNC_curl%%': `() { "${posix(process.execPath)}" "${posix(fake)}" "$@"; }`, FAKE_STORE_ROOT: dirs.store,
    CODETERM_CI_STORE_ENDPOINT: 'http://store.invalid', AWS_ACCESS_KEY_ID: 'id', AWS_SECRET_ACCESS_KEY: 'secret',
    GITHUB_ACTIONS: 'true', GITHUB_JOB: 'build', RUNNER_TEMP: dirs.temp, CI_ARTIFACT_SOURCES: sources };
  return {
    ...dirs,
    storeObject(key, body) { fs.mkdirSync(path.dirname(path.join(dirs.store, 'ci-artifacts', key)), { recursive: true }); fs.writeFileSync(path.join(dirs.store, 'ci-artifacts', key), body); },
    spooled(artifact, key, body) {
      const file = path.join(dirs.temp, 'ci-artifact-fallback', RUN, artifact, key);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, body);
    },
    failProbes(count) { fs.writeFileSync(path.join(dirs.store, '.probe-failures'), String(count)); },
    run(...args) {
      return execFileSync(bashInterpreter(), [posix(script), ...args.map(posix)], { env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    },
    sources: () => fs.readFileSync(sources, 'utf8').split('\n').filter(Boolean).sort(),
  };
}

test('a store-up prefix read returns store objects and spooled objects with their true locations', t => {
  const f = fixture(t);
  f.storeObject(`${PREFIX}domios.tar.gz`, 'store tarball');
  f.spooled('ci-artifact-spool-build-1-9', `${PREFIX}updater-darwin-x86_64.json`, 'spooled updater');
  f.run('get-prefix', PREFIX, f.out);
  assert.deepEqual(fs.readdirSync(f.out).sort(), ['domios.tar.gz', 'updater-darwin-x86_64.json']);
  assert.equal(fs.readFileSync(path.join(f.out, 'updater-darwin-x86_64.json'), 'utf8'), 'spooled updater');
  assert.deepEqual(f.sources(), [
    `${PREFIX}domios.tar.gz\tprivate-store:ci-artifacts/${PREFIX}domios.tar.gz`,
    `${PREFIX}updater-darwin-x86_64.json\trun-artifact:ci-artifact-spool-build-1-9/${PREFIX}updater-darwin-x86_64.json`,
  ]);
});

test('a store-up single read finds an object that only a run artifact holds', t => {
  const f = fixture(t);
  const key = `${PREFIX}updater-darwin-x86_64.json`;
  f.spooled('ci-artifact-spool-build-1-9', key, 'spooled updater');
  f.run('get', key, path.join(f.out, 'updater.json'));
  assert.equal(fs.readFileSync(path.join(f.out, 'updater.json'), 'utf8'), 'spooled updater');
  assert.deepEqual(f.sources(), [`${key}\trun-artifact:ci-artifact-spool-build-1-9/${key}`]);
});

test('one failed reachability probe does not spool a put', t => {
  const f = fixture(t);
  const key = `${PREFIX}updater-darwin-x86_64.json`;
  const file = path.join(f.out, 'updater.json');
  fs.writeFileSync(file, 'fresh');
  f.failProbes(1);
  f.run('put', key, file);
  assert.equal(fs.existsSync(path.join(f.temp, 'ci-artifact-spool')), false);
  assert.deepEqual(f.sources(), [`${key}\tprivate-store:ci-artifacts/${key}`]);
});

function duplicate(t, { storeAt, artifactAt }) {
  const cache = fs.mkdtempSync(path.join(os.tmpdir(), 'ci-artifact-dup-'));
  t.after(() => fs.rmSync(cache, { recursive: true, force: true }));
  const key = `${PREFIX}updater-darwin-x86_64.json`;
  const file = path.join(cache, 'ci-artifact-spool-build-2-7', key);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, 'spooled');
  const out = path.join(cache, 'out');
  const sources = path.join(cache, 'sources.tsv');
  const inventory = async () => [{ id: 7, name: 'ci-artifact-spool-build-2-7', expired: false, created_at: new Date(artifactAt).toISOString() }];
  const read = () => materializeFallback(cache, PREFIX, out, true, inventory,
    { store: [{ key, modified: storeAt }], runnerTemp: path.join(cache, 'no-runner-temp'), sources });
  return { key, out, read, sources: () => fs.existsSync(sources) ? fs.readFileSync(sources, 'utf8') : '' };
}

test('a duplicate whose store copy is newer is fetched from the store', async t => {
  const hour = 3_600_000;
  const d = duplicate(t, { storeAt: Date.UTC(2026, 10, 7, 17), artifactAt: Date.UTC(2026, 10, 7, 17) - hour });
  assert.deepEqual(await d.read(), [d.key]);
  assert.equal(fs.existsSync(path.join(d.out, 'updater-darwin-x86_64.json')), false);
  assert.equal(d.sources(), '');
});

test('a duplicate whose run artifact is newer is taken from the artifact and recorded there', async t => {
  const hour = 3_600_000;
  const d = duplicate(t, { storeAt: Date.UTC(2026, 10, 7, 17) - hour, artifactAt: Date.UTC(2026, 10, 7, 17) });
  assert.deepEqual(await d.read(), []);
  assert.equal(fs.readFileSync(path.join(d.out, 'updater-darwin-x86_64.json'), 'utf8'), 'spooled');
  assert.equal(d.sources(), `${d.key}\trun-artifact:ci-artifact-spool-build-2-7/${d.key}\n`);
});

test('a duplicate whose copies are too close to order across clocks is refused', async t => {
  const at = Date.UTC(2026, 10, 7, 17);
  const d = duplicate(t, { storeAt: at, artifactAt: at - COPY_ORDER_SKEW_MS + 1000 });
  await assert.rejects(d.read(), /copy_order_ambiguous/);
  assert.equal(fs.existsSync(d.out), false);
});
