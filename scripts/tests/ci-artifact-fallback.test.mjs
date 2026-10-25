import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { parse } from 'yaml';
import { evaluateCondition } from './workflow-condition.mjs';
import { spoolOutputs } from '../ci-artifact-spool.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const slash = value => value.replaceAll('\\', '/');

function harness() {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'ci-artifact-fallback-'));
  const bin = path.join(dir, 'bin');
  const artifacts = path.join(dir, 'runner-temp', 'ci-artifact-fallback', '41');
  mkdirSync(bin);
  mkdirSync(artifacts, { recursive: true });
  const env = {
    ...process.env, PATH: `${slash(bin)}${path.delimiter === ';' ? ':' : path.delimiter}${process.env.PATH}`,
    GITHUB_ACTIONS: 'true', GITHUB_JOB: 'build', GITHUB_REPOSITORY: 'owner/repo',
    CODETERM_CI_STORE_ENDPOINT: 'http://127.0.0.1:9', RUNNER_TEMP: slash(path.join(dir, 'runner-temp')),
    CI_ARTIFACT_SOURCES: slash(path.join(dir, 'sources.tsv')), GH_TOKEN: 'fixture-token',
  };
  delete env.SCCACHE_ENDPOINT;
  const run = (args, extra = {}) => spawnSync(bash, ['scripts/ci-artifact.sh', ...args], { cwd: root, env: { ...env, ...extra }, encoding: 'utf8' });
  return { dir, artifacts, env, run };
}

test('an unreachable store spools a put as a run artifact and records where it lives', () => {
  const h = harness();
  try {
    const file = path.join(h.dir, 'receipt.json');
    writeFileSync(file, '{"ok":true}');
    const put = h.run(['put', '41/release-dry-run/linux-x86_64/receipt.json', slash(file)]);
    assert.equal(put.status, 0, put.stderr);
    const name = readFileSync(path.join(h.dir, 'runner-temp', 'ci-artifact-spool', '.name'), 'utf8').trim();
    assert.match(name, /^ci-artifact-spool-build-/);
    assert.match(put.stdout, new RegExp(`::warning::artifact backend selected; 41/release-dry-run/linux-x86_64/receipt.json is retained in run artifact ${name}`));
    assert.equal(readFileSync(path.join(h.dir, 'runner-temp', 'ci-artifact-spool', '41', 'release-dry-run', 'linux-x86_64', 'receipt.json'), 'utf8'), '{"ok":true}');
    assert.equal(readFileSync(path.join(h.dir, 'sources.tsv'), 'utf8'),
      `41/release-dry-run/linux-x86_64/receipt.json\trun-artifact:${name}/41/release-dry-run/linux-x86_64/receipt.json\n`);
  } finally { rmSync(h.dir, { recursive: true, force: true }); }
});

test('an unreachable store reads keys and prefixes back from the key run\'s spooled artifacts', () => {
  const h = harness();
  try {
    const spooled = path.join(h.artifacts, 'ci-artifact-spool-build-1', '41', 'rust-receipt');
    mkdirSync(spooled, { recursive: true });
    writeFileSync(path.join(spooled, 'a.json'), 'A');
    writeFileSync(path.join(spooled, 'b.log'), 'B');
    const out = path.join(h.dir, 'out');
    const prefix = h.run(['get-prefix', '41/rust-receipt/', slash(out)]);
    assert.equal(prefix.status, 0, prefix.stderr);
    assert.match(prefix.stdout, /got 2 object\(s\) under 41\/rust-receipt\/ from the run artifacts/);
    assert.equal(readFileSync(path.join(out, 'a.json'), 'utf8'), 'A');
    assert.equal(readFileSync(path.join(out, 'b.log'), 'utf8'), 'B');
    const single = h.run(['get', '41/rust-receipt/b.log', slash(path.join(h.dir, 'single.log'))]);
    assert.equal(single.status, 0, single.stderr);
    assert.equal(readFileSync(path.join(h.dir, 'single.log'), 'utf8'), 'B');
    assert.match(readFileSync(path.join(h.dir, 'sources.tsv'), 'utf8'), /^41\/rust-receipt\/a\.json\trun-artifact:ci-artifact-spool-build-1\/41\/rust-receipt\/a\.json$/m);
    const missing = h.run(['get', '41/rust-receipt/none.json', slash(path.join(h.dir, 'none'))]);
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr, /is in no run artifact of run 41/);
  } finally { rmSync(h.dir, { recursive: true, force: true }); }
});

test('the run-artifact fallback refuses without evidence or a run id instead of reporting success', () => {
  const h = harness();
  try {
    const noToken = h.run(['get-prefix', '41/rust-receipt/', slash(path.join(h.dir, 'out'))], { GH_TOKEN: '', GITHUB_TOKEN: '' });
    assert.notEqual(noToken.status, 0);
    assert.match(noToken.stderr, /must download|is in no run artifact/);
    const noRun = h.run(['get', 'shared/key', slash(path.join(h.dir, 'x'))]);
    assert.notEqual(noRun.status, 0);
    assert.match(noRun.stderr, /names no run id/);
    assert.equal(existsSync(path.join(h.dir, 'runner-temp', 'ci-artifact-spool')), false);
  } finally { rmSync(h.dir, { recursive: true, force: true }); }
});

test('every job that stores CI artifacts uploads its spool and every reader prefetches fallback artifacts', () => {
  const workflows = ['release-macos-build', 'release-linux', 'release-linux-arm64', 'release-windows', 'tests',
    'rust-warnings-summary', 'release-manifest-dry-run'];
  for (const name of workflows) {
    const text = readFileSync(path.join(root, '.github', 'workflows', `${name}.yml`), 'utf8');
    for (const [id, job] of Object.entries(parse(text).jobs)) {
      const steps = job.steps ?? [];
      const runs = steps.map(step => step.run ?? '').join('\n');
      const stores = /ci-artifact\.sh put|release-dry-run-manifest\.mjs capture/.test(runs) ||
        steps.some(step => step.uses === './.github/actions/rust-warnings');
      if (stores) {
        const spool = steps.findIndex(step => step.uses === './.github/actions/ci-artifact-spool');
        assert.ok(spool >= 0, `${name}/${id} uploads its spool`);
        assert.equal(steps[spool].if, 'always()', `${name}/${id}`);
      }
      for (const step of steps) {
        if (/ci-artifact\.sh get|release-dry-run-manifest\.mjs aggregate/.test(step.run ?? '')) {
          assert.ok(steps.some(candidate => candidate.uses?.startsWith('actions/download-artifact@') && candidate.with?.pattern === 'ci-artifact-spool-*'), name+'/'+id+'/'+step.name);
        }
      }
    }
  }
  const action = parse(readFileSync(path.join(root, '.github', 'actions', 'ci-artifact-spool', 'action.yml'), 'utf8'));
  const upload = action.runs.steps.find(step => step.uses?.startsWith('actions/upload-artifact@'));
  assert.match(upload.uses, /@[0-9a-f]{40}$/);
  assert.equal(upload.with['if-no-files-found'], 'error');
  const reader = action.runs.steps.find(step => step.id === 'spool');
  assert.equal(reader.shell, "${{ runner.os == 'Windows' && 'pwsh' || 'bash' }}");
  assert.equal(reader.run, 'node scripts/ci-artifact-spool.mjs');
});

test('the spool reader names the spooled artifact and stays silent without one', () => {
  const temp = mkdtempSync(path.join(os.tmpdir(), 'ci-artifact-spool-'));
  try {
    assert.equal(spoolOutputs(temp), '');
    const spool = path.join(temp, 'ci-artifact-spool');
    mkdirSync(spool);
    writeFileSync(path.join(spool, '.name'), 'ci-artifact-spool-opaque\n');
    assert.equal(spoolOutputs(temp), `name=ci-artifact-spool-opaque\npath=${spool}\n`);
  } finally { rmSync(temp, { recursive: true, force: true }); }
});
