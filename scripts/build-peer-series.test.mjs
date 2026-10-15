// Caller: skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { parseCheckRun, parseDispatchArgs, parseSeriesSpec, seriesSummary } from './build-peer-lib.mjs';
import { otherGatewayJobs } from './build-peer-series.mjs';

const root = path.resolve(import.meta.dirname, '..');
const fakeCli = path.join(root, 'scripts/tests/fixtures/build-peer/fake-codeterm.mjs');
const A = 'a'.repeat(40);
const spec = (bases) => ({ marker: 'PROBE_MARK', probe0: 'const _: &str = "PROBE_MARK_0";', probe1: 'const _: &str = "PROBE_MARK_1";', bases });

function git(cwd, ...args) {
  const result = spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', ...args], { cwd, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

test('series spec accepts plain areas and refuses unsafe or ambiguous probes', () => {
  const parsed = parseSeriesSpec(spec([{ label: 'BEFORE', sha: A, areas: [{ name: 'pty_io', file: 'src/a.rs' }] }]));
  assert.equal(parsed.bases[0].areas[0].file, 'src/a.rs');
  assert.throws(() => parseSeriesSpec(spec([{ sha: A, areas: [{ name: 'x', file: '../etc/passwd' }] }])), /repository-relative/);
  assert.throws(() => parseSeriesSpec({ ...spec([{ sha: A, areas: [{ name: 'x', file: 'a.rs' }] }]), probe1: 'const _: &str = "PROBE_MARK_0";' }), /differ/);
  assert.throws(() => parseSeriesSpec(spec([{ sha: 'abc', areas: [{ name: 'x', file: 'a.rs' }] }])), /full SHA/);
  assert.throws(() => parseDispatchArgs('check-series', ['--peer', 'ubunta']), /--spec FILE/);
  assert.equal(parseDispatchArgs('check-series', ['--spec', 's.json', '--peer', 'ubunta']).spec, 's.json');
});

test('check run parsing keeps every cargo Finished time, the wall and the gateway job', () => {
  const output = 'Build gateway job J1 log: /g/jobs/J1.log\ncheck-queue: sha=x lane=0 packages=-p codeterm-api cli=0 exit=0 wall=42s full-log=/g/jobs/J1.check.log\n';
  const full = '    Finished `dev` profile [unoptimized] target(s) in 30.25s\n    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 05s\n';
  const run = parseCheckRun(output, full);
  assert.deepEqual([run.wall, run.exit, run.gatewayJob, run.packages, run.finished], [42, 0, 'J1', '-p codeterm-api', [30.25, 65]]);
  assert.equal(parseCheckRun('check-queue: no Rust changes between a and b; nothing to check\n').skipped, true);
  assert.equal(parseCheckRun('check-queue: sha=x lane=0 packages= cli=0 exit=0 wall=1s full-log=/Users/k/Library/Application Support/g/J.check.log\n').fullLog,
    '/Users/k/Library/Application Support/g/J.check.log');
  const lines = seriesSummary([{ base_label: 'BEFORE', area: 'pty_io', seed: A, edit: A, runs: { warm: run, floor: run, edit: run } }],
    { expected: 2, stopped: null, resultsPath: '/r.json', otherGatewayJobs: [] });
  assert.equal(lines[0], 'series: areas=1/2 stopped=none results=/r.json');
  assert.match(lines[1], /^BEFORE pty_io seed=aaaaaaaaa edit=aaaaaaaaa warm=42s\[30.25\+65\] floor=42s\[30.25\+65\] edit=42s\[30.25\+65\] exit=0\/0\/0 jobs=J1,J1,J1$/);
  assert.equal(lines.at(-1), 'gateway-other=none');
  assert.deepEqual(otherGatewayJobs({ recent: [{ job_id: 'J1', started_at: '2026-01-01T00:00:10Z' }, { job_id: 'X', job: 'nextest', started_at: '2026-01-01T00:00:20Z' },
    { job_id: 'OLD', started_at: '2025-01-01T00:00:00Z' }] }, new Set(['J1']), Date.parse('2026-01-01T00:00:00Z')), ['X:nextest']);
});

function seriesFixture(t, buildScript) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'build-peer-series-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.mkdirSync(path.join(dir, 'src'));
  fs.writeFileSync(path.join(dir, 'src/a.rs'), 'pub fn a() {}\n');
  fs.writeFileSync(path.join(dir, 'build.sh'), buildScript);
  git(dir, 'init', '--quiet');
  git(dir, 'add', '-A');
  git(dir, 'commit', '--quiet', '-m', 'before');
  const before = git(dir, 'rev-parse', 'HEAD');
  fs.mkdirSync(path.join(dir, 'src/moved'));
  git(dir, 'mv', 'src/a.rs', 'src/moved/a.rs');
  git(dir, 'commit', '--quiet', '-m', 'after');
  const after = git(dir, 'rev-parse', 'HEAD');
  git(dir, 'checkout', '--quiet', '--detach', before);
  fs.mkdirSync(path.join(dir, '.git/codeterm-peer-job.lock'));
  const env = { ...process.env, CODETERM_PEER_CLI: fakeCli, FAKE_CODETERM_LOG: path.join(dir, 'cli.log'), RUNS_LOG: path.join(dir, 'runs.log').replace(/\\/g, '/') };
  const runSeries = (s) => spawnSync(process.execPath, [path.join(root, 'scripts/build-peer.mjs'), 'run-series', '--id', 'peer-s',
    '--peer-name', 'ubunta', '--verb', 'check-series', '--sha', before, '--reply-peer', 'origin-id', '--reply-tab', 'tab01',
    '--deadline-secs', '120', '--spec-b64', Buffer.from(JSON.stringify(s)).toString('base64')], { cwd: dir, env, encoding: 'utf8' });
  const event = () => JSON.parse(fs.readFileSync(env.FAKE_CODETERM_LOG, 'utf8').trim().split('\n').at(-1));
  return { dir, before, after, env, runSeries, event };
}

// The marker is spelled in two halves so the fake build script itself never matches git grep.
const FAKE_CHECK = `n=$(( $(wc -l < "$RUNS_LOG" 2>/dev/null || echo 0) + 1 ))
mark=PROBE_; mark="\${mark}MARK_[01]"
printf '%s %s %s\\n' "$1" "$2" "$(git show "$2" | grep -o "$mark" | tail -1)" >> "$RUNS_LOG"
mkdir -p .git/g/jobs
dir="$(cygpath -m "$PWD" 2>/dev/null || echo "$PWD")"
printf '    Finished \`dev\` profile [unoptimized] target(s) in %s.50s\\n' "$n" > .git/g/jobs/J$n.check.log
echo "Build gateway job J$n log: $dir/.git/g/jobs/J$n.log"
echo "check-queue: sha=$2 lane=0 packages=-p x cli=0 exit=0 wall=\${n}s full-log=$dir/.git/g/jobs/J$n.check.log"
`;

test('series runs seed, floor and edit per area on scratch commits, then leaves the checkout clean at its start', (t) => {
  const fx = seriesFixture(t, FAKE_CHECK);
  const result = fx.runSeries(spec([
    { label: 'BEFORE', sha: fx.before, areas: [{ name: 'a', file: 'src/a.rs' }] },
    { label: 'AFTER', sha: fx.after, areas: [{ name: 'a', file: 'src/moved/a.rs' }] },
  ]));
  assert.equal(result.status, 0, result.stdout.slice(-1500) + result.stderr);
  const runs = fs.readFileSync(fx.env.RUNS_LOG, 'utf8').trim().split('\n').map((l) => l.split(' '));
  assert.equal(runs.length, 6);
  assert.ok(runs.every(([verb]) => verb === 'check-queue'));
  assert.equal(runs[0][1], runs[1][1], 'the floor re-checks the seed');
  assert.deepEqual(runs.map((r) => r[2]), ['PROBE_MARK_0', 'PROBE_MARK_0', 'PROBE_MARK_1', 'PROBE_MARK_0', 'PROBE_MARK_0', 'PROBE_MARK_1']);
  assert.equal(git(fx.dir, 'rev-parse', `${runs[2][1]}^`), runs[0][1], 'the edit commit sits on the seed');
  assert.equal(git(fx.dir, 'rev-parse', `${runs[0][1]}^`), fx.before);
  assert.equal(git(fx.dir, 'rev-parse', `${runs[3][1]}^`), fx.after);
  assert.equal(git(fx.dir, 'rev-parse', 'HEAD'), fx.before);
  assert.equal(git(fx.dir, 'status', '--porcelain', '--untracked-files=no'), '');
  assert.equal(spawnSync('git', ['grep', '-q', 'PROBE_MARK'], { cwd: fx.dir }).status, 1);
  assert.ok(!fs.existsSync(path.join(fx.dir, '.git/codeterm-peer-job.lock')));
  const event = fx.event();
  assert.match(event.stdin, /^\[peer-job peer-s\] peer=ubunta verb=check-series sha=\w{40} outcome=passed exit=0/);
  assert.match(event.stdin, /\nseries: areas=2\/2 stopped=none results=\S+peer-s\.series\.json\n/);
  assert.match(event.stdin, /\nBEFORE a seed=\w{9} edit=\w{9} warm=1s\[1\.5\] floor=2s\[2\.5\] edit=3s\[3\.5\] exit=0\/0\/0 jobs=J1,J2,J3\n/);
  assert.match(event.stdin, /\nAFTER a .* edit=6s\[6\.5\] exit=0\/0\/0 jobs=J4,J5,J6\n/);
  const saved = JSON.parse(fs.readFileSync(path.join(fx.dir, '.git/codeterm-peer-jobs/peer-s.series.json'), 'utf8'));
  assert.equal(saved.results[1].runs.edit.finished[0], 6.5);
});

test('series stops at the first skipped or failed run and reports partial results', (t) => {
  const fx = seriesFixture(t, `printf '%s\\n' "$2" >> "$RUNS_LOG"\necho "check-queue: no Rust changes between x and $2; nothing to check"\n`);
  const result = fx.runSeries(spec([{ label: 'BEFORE', sha: fx.before, areas: [{ name: 'a', file: 'src/a.rs' }, { name: 'b', file: 'src/a.rs' }] }]));
  assert.equal(result.status, 1);
  assert.equal(fs.readFileSync(fx.env.RUNS_LOG, 'utf8').trim().split('\n').length, 1);
  assert.match(fx.event().stdin, /outcome=failed exit=1 .*\nseries: areas=1\/2 stopped=skipped_no_rust_changes:a\/warm /);
  assert.equal(git(fx.dir, 'rev-parse', 'HEAD'), fx.before);
  assert.equal(git(fx.dir, 'status', '--porcelain', '--untracked-files=no'), '');
});
