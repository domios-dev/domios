import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { authorizeClosure, closeVerifiedIssues } from '../check-release-closure.mjs';

const repository = 'fixture/source';
const sha = 'a'.repeat(40), previous = 'b'.repeat(40), older = 'c'.repeat(40);
const route = suffix => `repos/${repository}/${suffix}`;
function fixture() {
  const release = { draft: false, published_at: '2026-10-01', assets: [
    { id: 11, name: 'issue-closure-boundary.json' }, { id: 12, name: 'domios-latest.json' }] };
  return { version: '1.16.1', sha, expected_start: previous,
    candidates: [{ version: '1.16.0', sha: previous }, ...Array.from({ length: 100 }, (_, i) => ({ version: `1.15.${i}`, sha: older }))],
    commits: [{ sha, url: `https://github.com/${repository}/commit/${sha}`, body: 'Public-Issue: rollacode/codeterm-releases#999' }],
    responses: {
      [route('releases/tags/v1.16.0')]: release,
      'repos/rollacode/codeterm-releases/releases/tags/v1.16.0': release,
      [route('releases/assets/11')]: { source_sha: previous, public_mirror_verified: true, validator_run_id: 50 },
      [route('actions/runs/50')]: { id: 50, path: '.github/workflows/release.yml', conclusion: 'success' },
      [route(`commits/${sha}/pulls?per_page=100&page=1`)]: [],
      'repos/rollacode/codeterm-releases/issues/999': { state: 'open' },
    } };
}
function resolve(f) {
  const run = spawnSync(process.platform === 'win32' ? 'python' : 'python3',
    ['scripts/tests/fixtures/release-issue-resolution.py'], {
      input: JSON.stringify(f), encoding: 'utf8', env: { ...process.env, GITHUB_REPOSITORY: repository },
    });
  assert.equal(run.status, 0, run.stderr);
  return JSON.parse(run.stdout);
}

test('canonical version tags supersede historical aliases and use fully qualified refs', () => {
  const result = resolve({ sha, tags: ['release-1.10.16', 'v1.10.16', 'release-1.7.6', 'v1.17.0-beta', 'unrelated'],
    objects: { 'refs/tags/v1.10.16^{commit}': previous, 'refs/tags/release-1.7.6^{commit}': older } });
  assert.equal(result.error, undefined);
  assert.deepEqual(result.result, [{ version: '1.10.16', sha: previous }, { version: '1.7.6', sha: older }]);
  assert.deepEqual(result.calls[1], ['rev-parse', 'refs/tags/v1.10.16^{commit}', 'refs/tags/release-1.7.6^{commit}']);
});

test('conflicting canonical boundaries still refuse before remote history work', () => {
  const f = fixture();
  f.candidates.unshift({ version: '1.16.0', sha: older });
  const result = resolve(f);
  assert.match(result.error, /conflicting reachable release tags for 1.16.0/);
  assert.deepEqual(result.calls, []);
});

test('newest verified reachable boundary stops remote history work', () => {
  const result = resolve(fixture());
  assert.equal(result.error, undefined);
  assert.equal(result.result.start, previous);
  assert.equal(result.calls.length, 6);
  assert.deepEqual(result.result.resolved.issues.map(row => row.reference), ['rollacode/codeterm-releases#999']);
  assert.equal(result.result.commits.length, 1);
});

test('unverified newer boundary is refused before selecting the previous verified publication', () => {
  const f = fixture();
  f.candidates.unshift({ version: '1.16.0', sha: previous });
  f.version = '1.17.0';
  f.candidates.unshift({ version: '1.16.1', sha });
  f.responses[route('releases/tags/v1.16.1')] = null;
  f.responses['repos/rollacode/codeterm-releases/releases/tags/v1.16.1'] = null;
  const result = resolve(f);
  assert.equal(result.error, undefined);
  assert.equal(result.result.history[0].version, '1.16.1');
  assert.equal(result.result.start, previous);
  assert.equal(result.calls.length, 8);
});

test('associated PR pagination retains every issue and exact PR metadata', () => {
  const f = fixture();
  const pr = n => ({ number: n, html_url: `https://github.com/${repository}/pull/${n}`,
    body: `Public-Issue: rollacode/codeterm-releases#${n}`, merged_at: '2026-10-20', merge_commit_sha: sha, base: { ref: 'develop' } });
  f.responses[route(`commits/${sha}/pulls?per_page=100&page=1`)] = Array.from({ length: 100 }, (_, i) => pr(i + 1));
  f.responses[route(`commits/${sha}/pulls?per_page=100&page=2`)] = [pr(101), { ...pr(102), merge_commit_sha: older }];
  for (let i = 1; i <= 101; i++) f.responses[`repos/rollacode/codeterm-releases/issues/${i}`] = { state: 'open' };
  const result = resolve(f);
  assert.equal(result.error, undefined);
  assert.equal(result.result.prs.length, 101);
  assert.equal(result.result.resolved.issues.length, 102);
  assert.deepEqual(result.result.prs[100], { number: 101, url: pr(101).html_url, body: pr(101).body, base_ref: 'develop' });
  assert.equal(result.calls.length, 108);
});

test('historical scan budget fails closed rather than selecting an unproven range', () => {
  const f = fixture();
  f.responses = {};
  for (const candidate of f.candidates) {
    for (const repo of [repository, 'rollacode/codeterm-releases']) f.responses[`repos/${repo}/releases/tags/v${candidate.version}`] = null;
  }
  const result = resolve(f);
  assert.match(result.error, /candidate bound/);
  assert.equal(result.calls.length, 100);
});

test('a boundary asset from a failed validator cannot cut off included issues', () => {
  const f = fixture();
  f.candidates = f.candidates.slice(0, 1);
  f.expected_start = null;
  f.responses[route('actions/runs/50')].conclusion = 'failure';
  const result = resolve(f);
  assert.equal(result.error, undefined);
  assert.equal(result.result.start, null);
  assert.match(result.result.history[0].refusal, /concluded failure/);
  assert.equal(result.result.resolved.issues.length, 1);
});

test('remote call budget also bounds included commit and issue lookups', () => {
  const f = fixture();
  f.commits = Array.from({ length: 1001 }, (_, i) => ({ sha: (i + 1).toString(16).padStart(40, '0'), body: '', url: `https://github.com/${repository}/commit/${i + 1}` }));
  for (const commit of f.commits) f.responses[route(`commits/${commit.sha}/pulls?per_page=100&page=1`)] = [];
  const result = resolve(f);
  assert.match(result.error, /remote-call bound/);
  assert.equal(result.calls.length, 1000);
});

test('full PR pages have a hard pagination bound', () => {
  const f = fixture();
  for (let page = 1; page <= 10; page++) f.responses[route(`commits/${sha}/pulls?per_page=100&page=${page}`)] = Array.from({ length: 100 }, () => ({ merged_at: null }));
  const result = resolve(f);
  assert.match(result.error, /page bound/);
  assert.equal(result.calls.length, 14);
});

function evidence(platforms, manifest) {
  return { version: '1.16.1', verified: true, cache_busted: true, failures: [], platforms,
    manifest: `https://github.com/rollacode/codeterm-releases/releases/download/v1.16.1/${manifest}?cb=1` };
}
const main = () => evidence(['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64', 'windows-aarch64', 'linux-x86_64', 'linux-aarch64'], 'domios-latest.json');
const deb = () => evidence(['linux-x86_64', 'linux-aarch64'], 'domios-latest-deb.json');
test('zero issue mutations until complete main and Debian served bytes verify', async () => {
  const closed = [], plan = [{ number: 999, comment: 'Released in v1.16.1\n\nhttps://github.com/fixture/source/commit/' + sha }];
  for (const [record, debRecord] of [[null, deb()], [main(), null], [{ ...main(), verified: false }, deb()],
    [{ ...main(), platforms: main().platforms.slice(1) }, deb()], [main(), { ...deb(), version: '1.16.0' }],
    [{ ...main(), cache_busted: false }, deb()], [main(), { ...deb(), platforms: ['linux-x86_64'] }],
    [{ ...main(), manifest: main().manifest.replace('/v1.16.1/', '/v1.16.0/') }, deb()],
    [{ ...main(), failures: ['signature mismatch'] }, deb()]]) {
    await assert.rejects(closeVerifiedIssues(plan, record, debRecord, '1.16.1', action => closed.push(action)));
    assert.equal(closed.length, 0);
  }
  authorizeClosure(main(), deb(), '1.16.1');
  await closeVerifiedIssues(plan, main(), deb(), '1.16.1', action => closed.push(action));
  assert.deepEqual(closed, plan);
});
