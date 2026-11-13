import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { bashInterpreter } from './bash-interpreter.mjs';
import { nativeIdentity } from './build-gateway-guard.mjs';

const unix = process.platform !== 'win32';
const gateway = path.resolve('scripts/build-gateway.sh');

for (const reentered of [false, true]) {
test(`gateway-cancel stops a waiting shell and compiler tree (reentered=${reentered})`, { timeout: 30000 }, async t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-cancel-'));
  const bash = bashInterpreter();
  const ownerScript = path.join(directory, 'owner.sh');
  const compilerScript = path.join(directory, 'compiler.mjs');
  fs.writeFileSync(ownerScript, `set -eu
if [ "\${3:-}" = parent ]; then
  source "$(dirname "$1")/dev-notify.sh"
  report_alert_once() { :; }
  status=0
  report_run_stream "$2/marker" '' compiler compile "$2/stream.log" "$BASH" "$0" "$1" "$2" child || status=$?
  printf 'PARENT_RESULT %s\\n' "$status"
  exit "$status"
fi
source "$1"
ct_gateway_set_fixture_dir "$2"
ct_gateway_acquire waiting-compiler
mkfifo "$2/output.fifo"
reader() { tee -a "$1"; }
reader "$2/output.log" < "$2/output.fifo" &
printf '%s\\n' "$!" > "$2/reader.pid"
exec > "$2/output.fifo" 2>&1
# Node must not set O_NONBLOCK on the caller's shared stdin pipe on macOS.
node "$2/compiler.mjs" < /dev/null
`);
  fs.writeFileSync(compilerScript, `import { spawn } from 'node:child_process';
const child = spawn(process.execPath, ['-e', 'process.stdin.resume(); console.log(process.pid)'], {
  detached: process.platform !== 'win32', stdio: ['pipe', 'pipe', 'inherit'],
});
child.stdout.once('data', data => console.log('READY', process.pid, String(data).trim()));
process.stdin.resume();
`);
  const launcher = spawn(bash, ['-c', `"$BASH" "$@"
status=$?
printf 'CALLER_RESULT %s\\n' "$status"
IFS= read -r release
exit "$status"`, 'caller', ownerScript,
    gateway.replaceAll('\\', '/'), directory.replaceAll('\\', '/'), reentered ? 'parent' : 'child'], {
    stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, FORCE_COLOR: '0' },
  });
  const exited = once(launcher, 'exit');
  const closed = once(launcher, 'close');
  let errors = '';
  launcher.stderr.on('data', chunk => { errors += chunk; });
  t.after(async () => {
    if (launcher.exitCode === null && launcher.signalCode === null) {
      if (process.platform === 'win32') spawnSync('taskkill.exe', ['/PID', String(launcher.pid), '/T', '/F']);
      else launcher.kill('SIGTERM');
    }
    await closed;
    fs.rmSync(directory, { recursive: true, force: true });
  });
  const lines = readline.createInterface({ input: launcher.stdout })[Symbol.asyncIterator]();
  let ready;
  do {
    ready = await lines.next();
    assert.equal(ready.done, false, errors);
  } while (!ready.value.startsWith('READY '));
  assert.match(ready.value, /^READY \d+ \d+$/);
  const descendants = ready.value.split(' ').slice(1).map(Number);
  const record = fs.readdirSync(path.join(directory, 'jobs')).find(file => file.endsWith('.json'));
  const job = JSON.parse(fs.readFileSync(path.join(directory, 'jobs', record), 'utf8'));
  const cancelled = spawnSync(bash, ['-c', `source "$1"; ct_gateway_set_fixture_dir "$2"; ct_gateway_cancel "$3"`,
    'cancel', gateway.replaceAll('\\', '/'), directory.replaceAll('\\', '/'), job.job_id],
  { encoding: 'utf8', timeout: 20000 });
  assert.equal(cancelled.status, 0, cancelled.stdout + cancelled.stderr);
  assert.equal(fs.existsSync(path.join(directory, 'lock')), false, 'cancel returned with the slot held');
  const terminal = JSON.parse(fs.readFileSync(path.join(directory, 'jobs', record), 'utf8'));
  assert.equal(terminal.state, 'cancelled');
  assert.equal(terminal.outcome, 'cancelled');
  assert.ok(terminal.ended_at);
  assert.equal(fs.existsSync(path.join(directory, 'jobs', `${job.job_id}.cancel-request`)), false);
  let caller;
  do {
    caller = await lines.next();
    assert.equal(caller.done, false, errors);
  } while (!caller.value.startsWith('CALLER_RESULT '));
  const expectedExit = unix ? 137 : 130;
  assert.equal(caller.value, `CALLER_RESULT ${expectedExit}`, errors);
  assert.equal(launcher.exitCode, null, 'calling shell must survive the cancelled command');
  launcher.stdin.end('release\n');
  const [exitCode, exitSignal] = await exited;
  assert.equal(exitSignal, null);
  assert.equal(exitCode, expectedExit, errors);
  await closed;
  for (const pid of descendants) {
    if (process.platform === 'linux' && fs.existsSync(`/proc/${pid}/stat`)) {
      assert.equal(fs.readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ')[1].split(' ')[0], 'Z');
    } else assert.equal(nativeIdentity(pid).state, 'gone', `compiler ${pid} survived cancellation`);
  }
  const successor = spawnSync(bash, ['-c', `set -e; source "$1"; ct_gateway_set_fixture_dir "$2";
    ct_gateway_acquire successor; ct_gateway_finish 0`, 'successor', gateway.replaceAll('\\', '/'), directory.replaceAll('\\', '/')],
  { encoding: 'utf8', timeout: 15000, env: { ...process.env, CODETERM_BUILD_GATEWAY_MODE: 'fail-fast' } });
  assert.equal(successor.status, 0, successor.stdout + successor.stderr);
});
}
const shell = `
set -eu
source "$1"
ct_gateway_set_fixture_dir "$2"
ct_gateway_acquire simulated-build
mkfifo "$2/output.fifo"
tee "$2/output.log" < "$2/output.fifo" &
reader=$!
exec 3> "$2/output.fifo"
node -e 'process.stdin.resume()' <&0 &
compiler=$!
printf 'READY %s %s %s\\n' "$$" "$compiler" "$reader"
wait "$compiler" || true
printf 'SURVIVED\\n'
read -r release
`;

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGKILL']) {
  test(`surviving gateway parent releases after supervisor ${signal}`, { skip: !unix, timeout: 15000 }, async t => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ci-cancel-'));
    const collector = spawn(process.execPath, ['scripts/collect-rust-warnings.mjs', 'run', '--log',
      path.join(directory, 'warnings.log'), '--', 'bash', '-c', shell, 'orphan', gateway, directory],
    { stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, CODETERM_BUILD_GATEWAY_MODE: 'fail-fast' } });
    const exited = once(collector, 'exit');
    const closed = once(collector, 'close');
    const lines = readline.createInterface({ input: collector.stdout })[Symbol.asyncIterator]();
    let errors = '';
    collector.stderr.on('data', chunk => { errors += chunk; });
    t.after(async () => {
      collector.kill('SIGKILL');
      await closed;
      fs.rmSync(directory, { recursive: true, force: true });
    });
    const next = async prefix => {
      for (;;) {
        const line = await lines.next();
        assert.equal(line.done, false, errors);
        if (line.value.startsWith(prefix)) return line.value;
      }
    };
    const [, owner, compiler, reader] = (await next('READY ')).split(' ').map(Number);
    assert.equal(fs.readFileSync(path.join(directory, 'lock/pid'), 'utf8').trim(), String(owner));
    const group = pid => process.platform === 'linux'
      ? fs.readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ')[1].split(' ')[2]
      : spawnSync('ps', ['-p', String(pid), '-o', 'pgid='], { encoding: 'utf8' }).stdout.trim();
    assert.equal(group(owner), group(reader));
    assert.notEqual(group(owner), group(process.pid));
    process.kill(compiler, 'SIGKILL');
    await next('SURVIVED');
    process.kill(owner, 0);
    process.kill(reader, 0);
    collector.kill(signal);
    const [code, delivered] = await exited;
    if (signal === 'SIGKILL') assert.equal(delivered, signal);
    else assert.equal(code, 128 + os.constants.signals[signal], errors);
    await closed;
    const acquire = spawnSync('bash', ['-c', `set -e; source "$1"; ct_gateway_set_fixture_dir "$2";
      ct_gateway_acquire successor; printf 'ACQUIRED\\n'; ct_gateway_finish 0`, 'successor', gateway, directory],
    { encoding: 'utf8', timeout: 10000, env: { ...process.env, CODETERM_BUILD_GATEWAY_MODE: 'fail-fast' } });
    assert.equal(acquire.status, 0, acquire.stdout + acquire.stderr);
    assert.match(acquire.stdout, /ACQUIRED/);
    assert.equal(fs.existsSync(path.join(directory, 'lock')), false);
    const jobs = fs.readdirSync(path.join(directory, 'jobs')).filter(file => file.endsWith('.json'))
      .map(file => JSON.parse(fs.readFileSync(path.join(directory, 'jobs', file), 'utf8')));
    assert.equal(jobs.find(job => job.command === 'simulated-build').outcome, 'owner_lost');
    assert.equal(jobs.find(job => job.command === 'successor').state, 'completed');
  });
}

for (const code of [0, 23]) {
  test(`collector preserves command exit ${code}`, { timeout: 10000 }, () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ci-result-'));
    try {
      const result = spawnSync(process.execPath, ['scripts/collect-rust-warnings.mjs', 'run', '--log',
        path.join(directory, 'warnings.log'), '--', process.execPath, '-e', `console.log('captured'); process.exit(${code})`],
      { encoding: 'utf8', timeout: 8000 });
      assert.equal(result.status, code, result.stderr);
      assert.match(result.stdout, /captured/);
      assert.match(fs.readFileSync(path.join(directory, 'warnings.log'), 'utf8'), /captured/);
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
}

const cappedScript = `
fake_nextest() { cat "$FAKE_OUTPUT"; return "\${FAKE_STATUS:-0}"; }
export -f fake_nextest
exec "$BASH" scripts/test-capped.sh "$@"
`;

function capped(output, args, status = 0) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'nextest-summary-'));
  try {
    const file = path.join(directory, 'output.log');
    fs.writeFileSync(file, output);
    return spawnSync(bashInterpreter(), ['-c', cappedScript, 'capped', ...args], {
      encoding: 'utf8', timeout: 10000,
      env: { ...process.env, CARGO_TEST_CMD: 'fake_nextest', CARGO_TEST_BUILD_CMD: '',
        CODETERM_TEST_HOST_TRIPLE: 'x86_64-unknown-linux-gnu', TEST_CAP_SECS: '0',
        CT_BUILD_ARTIFACT_DIR: '', FAKE_OUTPUT: file.replaceAll('\\', '/'), FAKE_STATUS: String(status) },
    });
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}

// https://nexte.st/docs/features/stress-tests/#stress-test-output
const stressOutput = Array.from({ length: 30 }, (_, i) =>
  ` Stress test [   0.006s] iteration ${i + 1}/30: 3 tests run: 3 passed, 49 skipped`).join('\n')
  + '\n     Summary [   0.200s] 30/30 stress run iterations: 30 passed\n';

for (const args of [
  ['--stress-count', '30'], ['--stress-duration', '1s'], ['--retries', '2'],
  ['--failure-output', 'final'], ['--profile', 'ci'], ['--stress-count=30'],
]) {
  test(`nextest option values are not filters: ${args.join(' ')}`, () => {
    const result = capped('Summary [0.001s] 0 tests run: 0 passed\n', args);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.doesNotMatch(result.stdout, /ZERO MATCHES/);
  });
}

for (const args of [
  ['--stress-count', '30', 'owned_test'], ['owned_test', '--stress-count', '30'],
  ['-E', 'test(owned_test)', '--stress-count', '30'],
]) {
  test(`nextest stress executes 90 passing runs: ${args.join(' ')}`, () => {
    const result = capped(stressOutput, args);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.match(result.stdout, /30\/30 stress run iterations: 30 passed/);
    assert.doesNotMatch(result.stdout, /ZERO MATCHES/);
  });
}

test('nextest stress preserves a failed command result', () => {
  const result = capped(stressOutput.replace('30 passed\n', '29 passed, 1 failed\n'), ['--stress-count', '30', 'owned_test'], 100);
  assert.equal(result.status, 100, result.stdout + result.stderr);
});

for (const output of ['', 'Summary [0.001s] 0 tests run: 0 passed\n',
  'Stress test [0.001s] iteration 1/1: 0 tests run: 0 passed, 3 skipped\nSummary [0.001s] 1/1 stress run iterations: 1 passed\n']) {
  test(`nextest still rejects empty filtered execution: ${JSON.stringify(output)}`, () => {
    const result = capped(output, ['--stress-count', '30', 'owned_test']);
    assert.equal(result.status, 97, result.stdout + result.stderr);
    assert.match(result.stdout, /ZERO MATCHES: filter owned_test/);
  });
}

test('nextest explicit empty-run policy is recognized after a filter', () => {
  const result = capped('', ['owned_test', '--stress-count', '30', '--no-tests', 'pass']);
  assert.equal(result.status, 0, result.stdout + result.stderr);
});

const loggingJob = `set -eu
source "$1"
source "$(dirname "$1")/build-gateway-logging.sh"
ct_gateway_set_fixture_dir "$2"
ct_cache_retention() { :; }
enter_stable_build_path() { :; }
release_stable_build_path() { :; }
CMD=gateway-fake
enter_build_gateway fake-evidence
exec 7>&1
node -e 'process.stdout.write("x".repeat(256 * 1024) + "\\nplugins receipt\\nBiome receipt\\ndiff-check receipt\\n"); process.stderr.write("stderr receipt\\n")'
if [ "$3" = handoff ]; then
  complete_build_gateway_before_handoff
else
  exit "$3"
fi
`;

for (const [mode, fingerprints] of [
  ['0', false], ['23', false], ['handoff', false], ['stuck', false], ['open-failed', false],
  ['0', true], ['stuck-handoff', true],
]) {
  test(`gateway drains evidence or fails closed (${mode}, fingerprints=${fingerprints})`, { timeout: 20000 }, () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-evidence-'));
    try {
      const failedReader = mode.startsWith('stuck') || mode === 'open-failed';
      const result = spawnSync(bashInterpreter(), ['-c', loggingJob, 'fake', gateway.replaceAll('\\', '/'),
        directory.replaceAll('\\', '/'), mode.startsWith('stuck') || mode === 'open-failed' ? (mode === 'stuck-handoff' ? 'handoff' : '0') : mode], {
        encoding: 'utf8', timeout: 15000, maxBuffer: 2 * 1024 * 1024,
        env: { ...process.env, CODETERM_BUILD_GATEWAY_MODE: 'fail-fast',
          CODETERM_BUILD_FINGERPRINTS: fingerprints ? '1' : '0',
          CT_BUILD_GATEWAY_READER_TIMEOUT_MS: '1000', CT_BUILD_GATEWAY_STUCK_READER: mode.startsWith('stuck') ? '1' : '0',
          CT_BUILD_GATEWAY_FAIL_LOG_OPEN: mode === 'open-failed' ? '1' : '0', CT_BUILD_GATEWAY_NO_TEE: '0' },
      });
      const diagnostic = result.stdout?.slice(-3000) + result.stderr;
      assert.equal(result.error, undefined, diagnostic);
      assert.equal(result.status, failedReader ? 74 : mode === '23' ? 23 : 0, diagnostic);
      const record = fs.readdirSync(path.join(directory, 'jobs')).find(file => file.endsWith('.json'));
      const job = JSON.parse(fs.readFileSync(path.join(directory, 'jobs', record), 'utf8'));
      assert.equal(job.state, failedReader || mode === '23' ? 'failed' : 'completed');
      assert.equal(job.outcome, failedReader ? 'logging_failed' : mode === '23' ? 'command_failed' : 'success');
      assert.equal(job.command_exit_code, mode === 'open-failed' ? null : mode === '23' ? 23 : 0);
      assert.equal(fs.existsSync(path.join(directory, 'lock')), false);
      if (failedReader) assert.match(result.stderr, /ERROR: build gateway logging path broke/);
      else {
        const log = fs.readFileSync(job.log_path, 'utf8');
        assert.match(log, /plugins receipt\nBiome receipt\ndiff-check receipt\n/);
        assert.match(log, /stderr receipt/);
        assert.match(log, /Build gateway job .* (?:build )?exit: (?:0|23)/);
        assert.ok(result.stdout.includes(log), diagnostic);
      }
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
}
