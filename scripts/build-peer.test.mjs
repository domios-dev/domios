// Caller: skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import {
  findPeer, jobEvent, loadPeers, pathPrefix, outcomeOf, parseDispatchArgs, parseJobLog, parseStatusOutput, pickPeer, remoteCommand,
} from './build-peer-lib.mjs';

const root = path.resolve(import.meta.dirname, '..');
const fixtures = path.join(root, 'scripts/tests/fixtures/build-peer');
const fakeCli = path.join(fixtures, 'fake-codeterm.mjs');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const SHA = 'a'.repeat(40);
const config = loadPeers(JSON.parse(fs.readFileSync(path.join(root, '.codeterm/build-machine/peers.json'), 'utf8')), {});
const peer = (name) => config.peers.find((p) => p.name === name);
const idle = (extra = {}) => ({ gateway: { state: 'idle', active: false, queue: 0, last_ended_at: null, ...extra }, job_lock: false });

function posixPath(p) {
  return process.platform === 'win32' ? `/${p[0].toLowerCase()}${p.slice(2).replace(/\\/g, '/')}` : p;
}

function tempDir(name) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `build-peer-${name}-`));
}

function run(cmd, args, opts = {}) {
  const result = spawnSync(cmd, args, { encoding: 'utf8', windowsHide: true, ...opts });
  return { status: result.status, stdout: result.stdout ?? '', stderr: result.stderr ?? '' };
}

function git(cwd, ...args) {
  const result = run('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', ...args], { cwd });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function calls(log) {
  return fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : [];
}

test('dispatch args keep check-queue flags, drop the peer route and require a peer', () => {
  const opts = parseDispatchArgs('check-queue', [SHA, '--peer', 'ROG', '--lane', '--max-errors', '5', '--touched-only']);
  assert.equal(opts.peer, 'ROG');
  assert.equal(opts.sha, SHA);
  assert.equal(opts.lane, true);
  assert.deepEqual(opts.remoteArgs, ['--lane', '--max-errors', '5', '--touched-only']);
  assert.throws(() => parseDispatchArgs('check-queue', [SHA]), /--peer/);
  assert.throws(() => parseDispatchArgs('check-queue', [SHA, '--peer', 'ROG', '--lane', '--all']), /exclusive/);
  const nextest = parseDispatchArgs('nextest', ['plan', 'test(store)', '--peer', 'auto', '--sha', SHA]);
  assert.deepEqual(nextest.remoteArgs, ['plan', 'test(store)']);
  assert.equal(nextest.sha, SHA);
  const linux = parseDispatchArgs('check-linux', []);
  assert.equal(linux.peer, 'auto');
  assert.equal(linux.os, 'linux');
  assert.throws(() => parseDispatchArgs('nextest', ['plan', '--peer', 'ROG', '--bogus']), /unknown option/);
});

test('peer config keeps eight unroutable until explicitly enabled', () => {
  assert.equal(peer('eight').routable, false);
  assert.equal(peer('ROG').shell, 'git-bash');
  const enabled = loadPeers(JSON.parse(fs.readFileSync(path.join(root, '.codeterm/build-machine/peers.json'), 'utf8')), { CODETERM_PEERS_ENABLE: 'eight' });
  assert.equal(enabled.peers.find((p) => p.name === 'eight').routable, true);
});

test('auto routing picks the least-loaded idle peer of the requested OS', () => {
  const candidates = [
    { peer: peer('ROG'), status: idle({ last_ended_at: '2026-10-15T10:00:00Z' }) },
    { peer: peer('ubunta'), status: idle({ last_ended_at: '2026-10-15T09:00:00Z' }) },
    { peer: peer('eight'), status: idle() },
  ];
  assert.equal(pickPeer(candidates, null).peer.name, 'ubunta');
  assert.equal(pickPeer(candidates, 'windows').peer.name, 'ROG');
  assert.equal(pickPeer(candidates, 'macos').error, 'no_peer');
  const busy = [
    { peer: peer('ROG'), status: { gateway: { state: 'running', active: true, queue: 2 }, job_lock: true } },
    { peer: peer('ubunta'), status: null, error: 'unreachable' },
  ];
  const refused = pickPeer(busy, null);
  assert.equal(refused.error, 'busy');
  assert.match(refused.detail, /ROG=running\/load4 ubunta=unknown\(unreachable\)/);
  const locked = [{ peer: peer('ROG'), status: { ...idle(), job_lock: true } }, { peer: peer('ubunta'), status: idle() }];
  assert.equal(pickPeer(locked, null).peer.name, 'ubunta');
});

test('status output is read from the marker line only', () => {
  const status = idle();
  assert.deepEqual(parseStatusOutput(`MSVC env loaded\r\nPEER_STATUS ${JSON.stringify(status)}\r\n$ `), status);
  assert.equal(parseStatusOutput('PEER_STATUS {broken'), null);
  assert.equal(parseStatusOutput('no marker'), null);
});

test('job log parsing returns the check-queue summary, receipts and errors once', () => {
  const parsed = parseJobLog(fs.readFileSync(path.join(fixtures, 'check-queue.log'), 'utf8'));
  assert.ok(parsed.summary.some((l) => l.startsWith('errors=1 ')));
  assert.ok(parsed.summary.some((l) => l.startsWith('warnings=2 shown=2 failing=codeterm-plan')));
  assert.ok(parsed.summary.some((l) => l.startsWith('check-queue: dev-lib ')));
  assert.equal(parsed.summary.filter((l) => l.startsWith('check-queue: app-test ')).length, 2);
  assert.ok(parsed.summary.at(-1).startsWith('check-queue: sha=1111111111111111111111111111111111111111'));
  assert.equal(parsed.details.length, 1);
  assert.match(parsed.gatewayLog, /20261015T101500-4242-1\.log$/);
});

test('job log parsing returns nextest counts and failing tests without ANSI', () => {
  const parsed = parseJobLog(fs.readFileSync(path.join(fixtures, 'nextest.log'), 'utf8'));
  assert.deepEqual(parsed.summary, [
    'Summary [   1.204s] 3 tests run: 2 passed, 1 failed, 0 skipped',
    'tests: runs=1 exit=0      Summary [   1.066s] 110 tests run: 110 passed, 5 skipped artifact=/tmp/parallel.log',
    'Slow unit ratchet FAIL: baseline requires exact binary/test names and a measured duration over 1 second',
    'Build gateway job 20261015T060514-1614340-26772 exit: 1',
  ]);
  assert.deepEqual(parsed.details, ['FAIL [   0.020s] codeterm-plan store::tests::persists', 'error: test run failed']);
});

test('completion event leads with route and outcome and ends with the remote log', () => {
  const parsed = parseJobLog(fs.readFileSync(path.join(fixtures, 'check-workspace.log'), 'utf8'));
  const event = jobEvent({ jobId: 'peer-1', peer: 'ubunta', verb: 'check-linux', sha: SHA, outcome: outcomeOf(0, false),
    exitCode: 0, wallSecs: 390, parsed, logPath: '/home/r/peer.log' });
  const lines = event.trim().split('\n');
  assert.equal(lines[0], `[peer-job peer-1] peer=ubunta verb=check-linux sha=${SHA} outcome=passed exit=0 wall=390s`);
  assert.ok(lines.includes('errors=0 shown=0'));
  assert.ok(lines.some((l) => l.startsWith('check-workspace: sha=')));
  assert.match(lines.at(-1), /^remote-log=\/home\/r\/peer\.log gateway-log=\/home\/rollacode\//);
  assert.equal(outcomeOf(75, false), 'busy');
  assert.equal(outcomeOf(0, true), 'deadline');
  assert.equal(outcomeOf(101, false), 'failed');
});

function bootstrapFixture(t) {
  const dir = tempDir('bootstrap');
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const origin = path.join(dir, 'origin');
  fs.mkdirSync(path.join(origin, 'scripts'), { recursive: true });
  fs.writeFileSync(path.join(origin, 'scripts/build-peer.mjs'),
    "import fs from 'node:fs';\nfs.writeFileSync(process.env.STUB_RUNNER_LOG, JSON.stringify(process.argv.slice(2)));\n");
  git(dir, 'init', '--quiet', '--initial-branch=develop', origin);
  git(origin, 'add', '-A');
  git(origin, 'commit', '--quiet', '-m', 'base');
  const sha = git(origin, 'rev-parse', 'HEAD');
  const bin = path.join(dir, 'bin');
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, 'codeterm'), `#!/usr/bin/env bash\nnode ${JSON.stringify(fakeCli.replace(/\\/g, '/'))} "$@"\n`, { mode: 0o755 });
  const env = { ...process.env, FAKE_CODETERM_LOG: path.join(dir, 'cli.log'), STUB_RUNNER_LOG: path.join(dir, 'runner.json') };
  env.PATH = `${posixPath(bin)}:${process.env.PATH}`;
  const checkout = path.join(dir, 'peer', 'codeterm-peer');
  const target = { name: 'ROG', checkout: posixPath(checkout) };
  const command = (opts) => remoteCommand({ peer: target, repository: origin.replace(/\\/g, '/'), jobId: 'peer-1', verb: 'check-queue',
    sha, clone: false, runner: ['run-job', '--id', 'peer-1'], reply: { peer: 'origin-id', tab: 'tab01' }, ...opts });
  const runCommand = (cmd) => run(bash, ['-c', `export PATH=${JSON.stringify(env.PATH.split(path.delimiter).join(':'))}; ${cmd}`], { env, cwd: dir });
  return { dir, origin, sha, env, checkout, command, runCommand };
}

test('remote command reports not_setup, clones on setup, runs the job from the SHA and refuses a held lock', (t) => {
  const fx = bootstrapFixture(t);
  fx.runCommand(fx.command());
  let sent = calls(fx.env.FAKE_CODETERM_LOG).filter((c) => c.args[0] === 'send');
  assert.equal(sent.length, 1);
  assert.match(sent[0].args.at(-1), /^\[peer-job peer-1\] peer=ROG verb=check-queue sha=\w{40} outcome=not_setup exit=\d+ stage=checkout_dir$/);
  assert.deepEqual(sent[0].args.slice(0, 6), ['send', '--require-agent', '--tab', 'tab01', '--mesh', 'origin-id']);

  fx.runCommand(fx.command({ clone: true }));
  assert.deepEqual(JSON.parse(fs.readFileSync(fx.env.STUB_RUNNER_LOG, 'utf8')), ['run-job', '--id', 'peer-1']);
  assert.equal(git(fx.checkout, 'rev-parse', 'HEAD'), fx.sha);
  assert.equal(git(fx.checkout, 'remote', 'get-url', '--push', 'origin'), 'fetch-only.invalid');
  assert.ok(fs.existsSync(path.join(fx.checkout, '.git/codeterm-peer-job.lock')), 'the runner owns the lock');

  fs.rmSync(fx.env.STUB_RUNNER_LOG);
  fx.runCommand(fx.command());
  sent = calls(fx.env.FAKE_CODETERM_LOG).filter((c) => c.args[0] === 'send');
  assert.match(sent.at(-1).args.at(-1), /outcome=busy exit=75 stage=peer_lock$/);
  assert.ok(!fs.existsSync(fx.env.STUB_RUNNER_LOG));

  fs.rmSync(path.join(fx.checkout, '.git/codeterm-peer-job.lock'), { recursive: true });
  fx.runCommand(fx.command({ sha: 'b'.repeat(40) }));
  sent = calls(fx.env.FAKE_CODETERM_LOG).filter((c) => c.args[0] === 'send');
  assert.match(sent.at(-1).args.at(-1), /outcome=bootstrap_failed exit=\d+ stage=fetch$/);
  assert.ok(!fs.existsSync(path.join(fx.checkout, '.git/codeterm-peer-job.lock')), 'a failed bootstrap releases the lock');
});

test('a prerequisite patch becomes a local commit on the SHA before the runner, and a bad one fails cleanly', (t) => {
  const fx = bootstrapFixture(t);
  fx.runCommand(fx.command({ clone: true }));
  fs.rmSync(path.join(fx.checkout, '.git/codeterm-peer-job.lock'), { recursive: true });
  const diff = 'diff --git a/note.txt b/note.txt\nnew file mode 100644\n--- /dev/null\n+++ b/note.txt\n@@ -0,0 +1 @@\n+patched\n';
  fx.runCommand(fx.command({ prereqB64: Buffer.from(diff).toString('base64') }));
  assert.equal(git(fx.checkout, 'rev-parse', 'HEAD^'), fx.sha);
  assert.equal(git(fx.checkout, 'show', 'HEAD:note.txt'), 'patched');
  assert.equal(git(fx.checkout, 'status', '--porcelain'), '');
  fs.rmSync(path.join(fx.checkout, '.git/codeterm-peer-job.lock'), { recursive: true });
  fx.runCommand(fx.command({ prereqB64: Buffer.from('diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-missing\n+y\n').toString('base64') }));
  const sent = calls(fx.env.FAKE_CODETERM_LOG).filter((c) => c.args[0] === 'send');
  assert.match(sent.at(-1).args.at(-1), /outcome=bootstrap_failed exit=\d+ stage=prereq$/);
  assert.equal(git(fx.checkout, 'rev-parse', 'HEAD'), fx.sha);
  assert.equal(git(fx.checkout, 'status', '--porcelain', '--untracked-files=no'), '');
});

test('test-one dispatches its scope and name, and --prereq ships the diff without changing the job SHA', (t) => {
  const fx = dispatchFixture(t, {});
  const patch = path.join(fx.dir, 'fix.diff');
  fs.writeFileSync(patch, 'diff --git a/a b/a\n');
  const result = fx.dispatch('test-one', 'sentinel', 'group_retirement', '--peer', 'ubunta', '--sha', fx.sha, '--prereq', patch);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /prerequisite fix\.diff sha256=[0-9a-f]{16} is committed locally/);
  const cmd = calls(fx.log).find((c) => c.args[1] === 'new').args.at(-1);
  assert.ok(cmd.includes(`prereq ${Buffer.from('diff --git a/a b/a\n').toString('base64')};`));
  assert.match(cmd, /'--' 'test-one' 'sentinel' 'group_retirement'; fi; exit$/);
  const queue = fx.dispatch('check-queue', fx.sha, '--peer', 'ubunta', '--prereq', patch);
  assert.equal(queue.status, 0, queue.stderr);
  assert.match(calls(fx.log).filter((c) => c.args[1] === 'new').at(-1).args.at(-1), /'--' 'check-queue' 'HEAD'; fi; exit$/);
});

function dispatchFixture(t, statuses) {
  const dir = tempDir('dispatch');
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  git(dir, 'init', '--quiet', '--initial-branch=develop');
  git(dir, 'commit', '--quiet', '--allow-empty', '-m', 'base');
  const sha = git(dir, 'rev-parse', 'HEAD');
  git(dir, 'update-ref', 'refs/remotes/origin/lane', sha);
  git(dir, 'commit', '--quiet', '--allow-empty', '-m', 'unpushed');
  const unpushed = git(dir, 'rev-parse', 'HEAD');
  const env = { ...process.env, CODETERM_PEER_CLI: fakeCli, FAKE_CODETERM_LOG: path.join(dir, 'cli.log'),
    FAKE_PEER_STATUS: JSON.stringify(statuses), CODETERM_TAB_ID: 'tab01', CODETERM_PEERS_ENABLE: '' };
  const state = path.join(dir, 'state');
  const dispatch = (...args) => run(process.execPath, [path.join(root, 'scripts/build-peer.mjs'), 'dispatch', ...args, '--state-dir', state], { cwd: dir, env });
  return { dir, sha, unpushed, env, state, dispatch, log: env.FAKE_CODETERM_LOG };
}

test('named peer dispatch opens one bounded build tab on that peer and records the job', (t) => {
  const fx = dispatchFixture(t, {});
  const result = fx.dispatch('check-queue', fx.sha, '--peer', 'rog', '--lane');
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, new RegExp(`peer-job: id=peer-\\S+ peer=ROG verb=check-queue sha=${fx.sha} remote-tab=remote01 deadline=2700s`));
  const opened = calls(fx.log).filter((c) => c.args[0] === 'tab' && c.args[1] === 'new');
  assert.equal(opened.length, 1);
  const args = opened[0].args;
  const flag = (name) => args[args.indexOf(name) + 1];
  assert.equal(flag('--mesh'), 'ROG');
  assert.equal(flag('--shell'), 'git-bash');
  assert.ok(args.includes('--eager') && !args.includes('--timeout'));
  assert.equal(flag('--purpose'), 'build');
  assert.match(flag('--cmd'), new RegExp(`'run-job' .*'--reply-peer' 'a35724e2-c7e3-486b-95d1-5edaa7f8d749' '--reply-tab' 'tab01' .*'--' 'check-queue' '${fx.sha}'`));
  assert.doesNotMatch(flag('--cmd'), /'--peer'/);
  assert.match(flag('--cmd'), /'check-queue' '[0-9a-f]{40}' '--lane'; fi; exit$/);
  const records = fs.readdirSync(fx.state);
  assert.equal(records.length, 1);
  assert.equal(JSON.parse(fs.readFileSync(path.join(fx.state, records[0]), 'utf8')).remote_tab, 'remote01');
});

test('dispatch refuses an unpushed SHA, an unroutable peer and a missing reply tab before opening a tab', (t) => {
  const fx = dispatchFixture(t, {});
  assert.match(fx.dispatch('check-queue', fx.unpushed, '--peer', 'ROG').stderr, /push it/);
  assert.match(fx.dispatch('check-queue', fx.sha, '--peer', 'eight').stderr, /not routable/);
  fx.env.CODETERM_TAB_ID = '';
  assert.match(fx.dispatch('check-queue', fx.sha, '--peer', 'ROG').stderr, /--reply-tab/);
  assert.equal(calls(fx.log).filter((c) => c.args[1] === 'new').length, 0);
});

test('auto routing reads each peer status over mesh and refuses with 75 when none is idle', (t) => {
  const fx = dispatchFixture(t, { ROG: idle(), ubunta: { ...idle(), gateway: { ...idle().gateway, state: 'running', active: true } } });
  const result = fx.dispatch('nextest', 'plan', '--peer', 'auto', '--sha', fx.sha);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /peer-route: chose ROG; ROG=idle\/load0 ubunta=running\/load1/);
  const execs = calls(fx.log).filter((c) => c.args[1] === 'exec');
  assert.deepEqual(execs.map((c) => c.args[c.args.indexOf('--tab') + 1]).sort(), ['status-ROG', 'status-ubunta']);
  assert.match(calls(fx.log).find((c) => c.args[1] === 'new').args.at(-1), /'--' 'nextest' 'plan'; fi; exit$/);

  const busy = dispatchFixture(t, { ROG: 'exec_timeout', ubunta: 'not_setup' });
  const refused = busy.dispatch('check-queue', busy.sha, '--peer', 'auto');
  assert.equal(refused.status, 75);
  assert.match(refused.stderr, /busy; ROG=unknown\(exec_timeout\) ubunta=unknown\(not_setup\)/);

  const late = dispatchFixture(t, { ROG: 'exec_timeout', ubunta: { late: idle() } });
  const routed = late.dispatch('check-linux', late.sha);
  assert.equal(routed.status, 0, routed.stderr);
  assert.match(routed.stdout, /peer=ubunta verb=check-linux/);
});

test('check-linux routes to the Linux peer once per SHA unless forced', (t) => {
  const fx = dispatchFixture(t, { ROG: idle(), ubunta: idle() });
  const first = fx.dispatch('check-linux', fx.sha);
  assert.equal(first.status, 0, first.stderr);
  assert.match(first.stdout, /peer=ubunta verb=check-linux/);
  assert.match(calls(fx.log).find((c) => c.args[1] === 'new').args.at(-1), new RegExp(`'--' 'check-workspace' '${fx.sha}'; fi; exit$`));
  const again = fx.dispatch('check-linux', fx.sha);
  assert.equal(again.status, 0);
  assert.match(again.stdout, /already dispatched/);
  assert.equal(calls(fx.log).filter((c) => c.args[1] === 'new').length, 1);
  assert.equal(fx.dispatch('check-linux', fx.sha, '--force').status, 0);
  assert.equal(calls(fx.log).filter((c) => c.args[1] === 'new').length, 2);
});

function jobFixture(t, buildScript) {
  const dir = tempDir('job');
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  git(dir, 'init', '--quiet');
  fs.writeFileSync(path.join(dir, 'build.sh'), buildScript);
  fs.mkdirSync(path.join(dir, '.git/codeterm-peer-job.lock'));
  const env = { ...process.env, CODETERM_PEER_CLI: fakeCli, FAKE_CODETERM_LOG: path.join(dir, 'cli.log') };
  const runJob = (deadline, ...verb) => run(process.execPath, [path.join(root, 'scripts/build-peer.mjs'), 'run-job', '--id', 'peer-9',
    '--peer-name', 'ROG', '--verb', 'check-queue', '--sha', SHA, '--reply-peer', 'origin-id', '--reply-tab', 'tab01',
    '--deadline-secs', String(deadline), '--', ...verb], { cwd: dir, env });
  return { dir, env, runJob, event: () => calls(env.FAKE_CODETERM_LOG).find((c) => c.args[0] === 'send') };
}

test('peer job runs the verb with --wait-build, releases the lock, then sends one completion event', (t) => {
  const log = posixPath(path.join(fixtures, 'check-queue.log'));
  const fx = jobFixture(t, `echo "args=$*"\ncat ${JSON.stringify(log)}\nexit 101\n`);
  const result = fx.runJob(60, 'check-queue', SHA);
  assert.equal(result.status, 101);
  assert.match(result.stdout, new RegExp(`args=check-queue ${SHA} --wait-build`));
  const event = fx.event();
  assert.deepEqual(event.args, ['send', '--require-agent', '--stdin', '--tab', 'tab01', '--mesh', 'origin-id']);
  assert.match(event.stdin, /^\[peer-job peer-9\] peer=ROG verb=check-queue sha=a{40} outcome=failed exit=101 wall=\d+s\n/);
  assert.match(event.stdin, /\nwarnings=2 shown=2 failing=codeterm-plan\n/);
  assert.match(event.stdin, /\nremote-log=.*codeterm-peer-jobs[\\/]peer-9\.log gateway-log=/);
  assert.equal(event.lockHeld, false, 'the event cues the next dispatch, so the checkout is free when it is sent');
});

test('peer job past its deadline is killed and reported as deadline', (t) => {
  const fx = jobFixture(t, 'echo started\nnode -e "setInterval(() => {}, 1e6)"\n');
  const result = fx.runJob(1, 'nextest', 'plan');
  assert.notEqual(result.status, 0);
  assert.match(fx.event().stdin, /outcome=deadline/);
  assert.ok(!fs.existsSync(path.join(fx.dir, '.git/codeterm-peer-job.lock')));
});

test('peer-setup clones at the requested SHA and forwards the toolchain install request', (t) => {
  const fx = dispatchFixture(t, {});
  const setup = (...args) => run(process.execPath, [path.join(root, 'scripts/build-peer.mjs'), 'setup', ...args, '--state-dir', fx.state], { cwd: fx.dir, env: fx.env });
  assert.equal(setup('ROG', '--sha', fx.sha, '--install-toolchain').status, 0);
  assert.equal(setup('ubunta', '--sha', fx.sha).status, 0);
  const [rog, ubunta] = calls(fx.log).filter((c) => c.args[1] === 'new').map((c) => c.args.at(-1));
  assert.match(rog, /ct_clone\(\)/);
  assert.match(rog, new RegExp(`'setup-local' .*'--sha' '${fx.sha}' .*'--install-toolchain' '1'; fi; exit$`));
  assert.doesNotMatch(ubunta, /--install-toolchain/);
  assert.notEqual(setup('ROG', '--sha', fx.unpushed).status, 0);
});

test('a peer path list prefixes the job command and the status probe; node is checked before the runner', () => {
  const ubunta = findPeer(config.peers, 'ubunta');
  assert.deepEqual(ubunta.path, ['$HOME/.local/node22/bin']);
  assert.equal(pathPrefix(findPeer(config.peers, 'ROG')), '');
  const command = remoteCommand({ peer: ubunta, repository: 'https://example.invalid/r.git', jobId: 'peer-1', verb: 'check-linux',
    sha: 'a'.repeat(40), clone: false, runner: ['run-job'], reply: { peer: 'origin', tab: 'tab01' } });
  assert.ok(command.startsWith('export PATH="$HOME/.local/node22/bin:$PATH"; '));
  assert.ok(command.indexOf('ct_step node_missing command -v node') < command.indexOf('node scripts/build-peer.mjs'));
  assert.throws(() => loadPeers({ peers: { X: { os: 'linux', checkout: '/x', path: ['/bin; rm -rf /'] } } }), /plain directories/);
});
