// Caller: scripts ci-rust-tests.mjs, ci-rust-archives.mjs; scripts tests/ci-classifier-cache.test.mjs
// Classifies a Cargo target's tests once per source content: results are keyed by the digest of the
// classifier and of every file it can read, kept on the runner and in the private CI store.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const CLASSIFIER = path.join(ROOT, 'scripts', 'ci-test-classifier');

function filesUnder(directory) {
  if (!directory || !fs.existsSync(directory)) return [];
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) return entry.name === 'target' ? [] : filesUnder(full);
    return entry.isFile() ? [full] : [];
  }).sort();
}

// The classifier follows modules from the target's root file, so every file beside it can change the result.
export function classificationKey(source, generated, read = (file) => fs.readFileSync(file)) {
  const hash = createHash('sha256');
  const add = (label, files, base) => {
    for (const file of files) hash.update(`${label}:${path.relative(base, file).replaceAll('\\', '/')}\0`).update(read(file)).update('\0');
  };
  add('classifier', [...filesUnder(path.join(CLASSIFIER, 'src')), path.join(CLASSIFIER, 'Cargo.toml'), path.join(CLASSIFIER, 'Cargo.lock')], CLASSIFIER);
  const sourceDir = path.dirname(source);
  hash.update(`root:${path.basename(source)}\0`);
  add('source', filesUnder(sourceDir), sourceDir);
  if (generated) add('generated', filesUnder(generated), generated);
  return hash.digest('hex');
}

function cacheRoot(env) {
  if (env.CI_CLASSIFIER_CACHE) return env.CI_CLASSIFIER_CACHE;
  return env.CI_TARGET_ROOT ? path.join(env.CI_TARGET_ROOT, '..', 'classifier-cache') : path.join(os.tmpdir(), 'domios-classifier-cache');
}

function fromStore(key, destination, env) {
  if (!env.AWS_ACCESS_KEY_ID) return false;
  try {
    execFileSync('bash', ['scripts/ci-artifact.sh', 'get', `ci-classifier/${key}.json`, destination], { cwd: ROOT, stdio: 'ignore', env });
    return fs.existsSync(destination);
  } catch { return false; }
}

function toStore(key, file, env) {
  if (!env.AWS_ACCESS_KEY_ID) return;
  try { execFileSync('bash', ['scripts/ci-artifact.sh', 'put', `ci-classifier/${key}.json`, file], { cwd: ROOT, stdio: 'ignore', env }); } catch { /* A cache miss later only costs one classifier run. */ }
}

export function runClassifier(source, generated, env) {
  // One classifier build per machine, outside the lane targets and the checkout.
  const target = path.join(cacheRoot(env), 'classifier-target');
  return execFileSync('cargo', ['run', '--quiet', '--locked', '--manifest-path', path.join(CLASSIFIER, 'Cargo.toml'), '--', source, ...(generated ? [generated] : [])],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'], env: { ...env, CARGO_TARGET_DIR: target } });
}

export function classifyTarget(source, generated, { env = process.env, run = runClassifier, load = fromStore, save = toStore, stats } = {}) {
  const key = classificationKey(source, generated);
  const local = path.join(cacheRoot(env), `${key}.json`);
  if (fs.existsSync(local)) { if (stats) stats.local++; return JSON.parse(fs.readFileSync(local, 'utf8')); }
  fs.mkdirSync(path.dirname(local), { recursive: true });
  if (load(key, local, env)) { if (stats) stats.store++; return JSON.parse(fs.readFileSync(local, 'utf8')); }
  const output = run(source, generated, env);
  const entries = JSON.parse(output);
  fs.writeFileSync(local, output);
  save(key, local, env);
  if (stats) stats.classified++;
  return entries;
}
