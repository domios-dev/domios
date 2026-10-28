import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { scanText, scanRepository, fingerprint } from '../check-public-privacy.mjs';

const seeds = [
  ['tailnet-ip', '100.' + '87.43.219'],
  ['tailnet-dns', 'node.private-fixture.ts' + '.net'],
  ['tailnet-ipv6', 'fd7a:' + '115c:a1e0:ab12::9'],
  ['store-endpoint', 'http://' + 'minio.fixture.internal:9000'],
  ['store-endpoint', 'https://172.' + '20.1.7:9443'],
  ['personal-path', '/Users/' + 'PrivateFixtureOwner/project'],
  ['personal-path', 'C:\\Users\\' + 'PrivateFixtureOwner\\project'],
  ['personal-path', 'C:\\\\Users\\\\' + 'PrivateFixtureOwner\\\\project'],
  ['email', 'fixture-person@' + 'mail.internal'],
  ['machine-id', 'a2:b3:c4:' + 'd5:e6:f7'],
  ['machine-name', 'DESKTOP-' + 'SYNTHETIC9'],
  ['captured-report', 'STATUS: ' + 'in_review\nCHANGES: captured owner report'],
];

test('private categories fail synthetic seeds without printing their bodies', () => {
  for (const [rule, seed] of seeds) {
    const findings = scanText('capture.txt', `header\n${seed}`);
    assert.ok(findings.some(finding => finding.rule === rule), rule);
    assert.ok(findings.every(finding => finding.line === 2));
    assert.ok(!JSON.stringify(findings).includes(seed), rule);
  }
});

test('public, reserved and configuration material remains allowed', () => {
  for (const value of [
    'https://domios.app', 'https://github.com/domios-dev/domios',
    'http://store.invalid:9000', 'https://s3.example.com', 'http://localhost:9000',
    'http://127.0.0.1:7686', 'contact@example.org', '/Users/example/project',
    '/home/alice/project', '${CI_STORE_PUBLIC_ENDPOINT}', '${HOME}/.domios',
    'C:\\Users\\example\\project', 'git@github.com',
  ]) assert.deepEqual(scanText('guide.md', value), [], value);
});

test('release template delimiters do not create email findings', () => {
  const manifest = readFileSync(new URL('../../distribution/scoop/bucket/domios.json', import.meta.url), 'utf8');
  assert.deepEqual(scanText('manifest.json', manifest), []);
  assert.ok(scanText('manifest.json', manifest + seeds.find(([rule]) => rule === 'email')[1]).some(finding => finding.rule === 'email'));
});

test('a reviewed synthetic vector is allowed only for its exact path, rule and token', () => {
  const seed = seeds[0][1];
  const allowance = { path: 'fixture.txt', rule: 'tailnet-ip', sha256: fingerprint(seed), reason: 'Synthetic parser fixture, never contacted.' };
  assert.deepEqual(scanText('fixture.txt', seed, [allowance]), []);
  assert.equal(scanText('other.txt', seed, [allowance]).length, 1);
  assert.equal(scanText('fixture.txt', seed.replace(/219$/, '218'), [allowance]).length, 1);
  assert.equal(scanText('fixture.txt', seed, [{ ...allowance, reason: '' }]).length, 1);
  assert.equal(scanText('fixture.txt', seed, [{ ...allowance, rule: 'email' }]).length, 1);
});

test('the CLI scans only tracked current content and fails closed on a missing file', () => {
  const root = mkdtempSync(join(tmpdir(), 'public-privacy-'));
  try {
    execFileSync('git', ['init', '-q', root]);
    mkdirSync(join(root, 'scripts'));
    writeFileSync(join(root, 'scripts/public-privacy-allowlist.json'), '[]');
    writeFileSync(join(root, 'tracked.txt'), 'https://store.invalid:9000');
    writeFileSync(join(root, 'untracked.txt'), seeds[0][1]);
    execFileSync('git', ['add', 'tracked.txt', 'scripts'], { cwd: root });
    const run = () => spawnSync(process.execPath, [resolve('scripts/check-public-privacy.mjs')], { cwd: root, encoding: 'utf8' });
    assert.equal(run().status, 0);
    writeFileSync(join(root, 'tracked.txt'), seeds[0][1]);
    const rejected = run();
    assert.equal(rejected.status, 1);
    assert.match(rejected.stderr, /tracked.txt:1: tailnet-ip \[redacted\]/);
    assert.ok(!rejected.stderr.includes(seeds[0][1]));
    rmSync(join(root, 'tracked.txt'));
    assert.equal(run().status, 2);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('both CI guard jobs execute the same current-tree command', () => {
  for (const path of ['.github/workflows/tests.yml', '.github/workflows/tests-side.yml']) {
    assert.match(readFileSync(path, 'utf8'), /name: Current tracked tree privacy\s+run: node scripts\/check-public-privacy\.mjs/);
  }
});

test('streaming preserves a private token and line number across chunk boundaries', () => {
  const root = mkdtempSync(join(tmpdir(), 'public-privacy-stream-'));
  try {
    execFileSync('git', ['init', '-q', root]);
    const prefix = 'public\n' + ' '.repeat(64 * 1024 - 12);
    writeFileSync(join(root, 'capture.txt'), prefix + seeds[5][1] + '\n' + ' '.repeat(64 * 1024));
    execFileSync('git', ['add', 'capture.txt'], { cwd: root });
    const result = scanRepository(root, []);
    assert.equal(result.files, 1);
    assert.equal(result.findings.length, 1);
    assert.equal(result.findings[0].rule, 'personal-path');
    assert.equal(result.findings[0].line, 2);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
