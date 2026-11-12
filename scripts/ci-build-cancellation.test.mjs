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
node "$2/compiler.mjs"
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
