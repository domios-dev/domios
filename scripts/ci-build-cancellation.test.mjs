import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';

const unix = process.platform !== 'win32';
const gateway = path.resolve('scripts/build-gateway.sh');
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
