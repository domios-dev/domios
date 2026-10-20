// Caller: node --test scripts/build-gateway-relocate.test.mjs; skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const env = { ...process.env, PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH };

function gateway(body, ...args) {
  return spawnSync(bash, ['-c', `source scripts/build-gateway.sh; ${body}`, 'fixture', ...args], { cwd: root, env, encoding: 'utf8' });
}

function fixture(t) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-relocate-'));
  t.after(() => fs.rmSync(base, { recursive: true, force: true }));
  const legacy = path.join(base, 'CodeTerm', 'build-gateway');
  fs.mkdirSync(path.join(legacy, 'jobs'), { recursive: true });
  fs.mkdirSync(path.join(legacy, 'queue'));
  fs.writeFileSync(path.join(legacy, 'jobs', 'job-1.json'), '{}');
  return { base, legacy, target: path.join(base, 'domios-build-gateway') };
}

const posix = (value) => value.replaceAll('\\', '/');
const resolveWindows = (base) => gateway('base="$1"; ct_gateway_local_app_data() { printf "%s\\n" "$base"; }; OSTYPE=msys; ct_gateway_dir', base);
const move = (legacy, target) => gateway('CT_BUILD_GATEWAY_SCOPE=fixture ct_gateway_move_when_idle "$1" "$2"', legacy, target);

test('Windows gateway state resolves outside every per-user install folder', (t) => {
  const { base, legacy } = fixture(t);
  fs.rmSync(legacy, { recursive: true });
  const resolved = resolveWindows(base);
  assert.equal(resolved.status, 0, resolved.stderr);
  const dir = posix(resolved.stdout.trim());
  assert.equal(dir, posix(path.join(base, 'domios-build-gateway')));
  for (const install of ['CodeTerm', 'Domios']) {
    assert.ok(!dir.toLowerCase().startsWith(`${posix(path.join(base, install)).toLowerCase()}/`), dir);
  }
});

test('an unmoved legacy directory stays shared until relocation, then the target wins', (t) => {
  const { base, legacy, target } = fixture(t);
  assert.equal(posix(resolveWindows(base).stdout.trim()), posix(legacy));
  fs.mkdirSync(target);
  assert.equal(posix(resolveWindows(base).stdout.trim()), posix(target));
});

test('idle legacy state moves once by rename and releases the mutex at the target', (t) => {
  const { legacy, target } = fixture(t);
  const moved = move(legacy, target);
  assert.equal(moved.status, 0, moved.stderr);
  assert.ok(!fs.existsSync(legacy));
  assert.equal(fs.readFileSync(path.join(target, 'jobs', 'job-1.json'), 'utf8'), '{}');
  assert.ok(!fs.existsSync(path.join(target, 'state.lock')));
  assert.equal(move(legacy, target).status, 0);
});

test('a held build slot or queued caller defers the move and leaves the mutex free', (t) => {
  for (const busy of [['lock'], ['queue', 'ticket']]) {
    const { legacy, target } = fixture(t);
    fs.mkdirSync(path.join(legacy, ...busy), { recursive: true });
    const result = move(legacy, target);
    assert.equal(result.status, 75, result.stderr);
    assert.ok(fs.existsSync(path.join(legacy, 'jobs', 'job-1.json')));
    assert.ok(!fs.existsSync(target));
    assert.ok(!fs.existsSync(path.join(legacy, 'state.lock')));
  }
});

test('legacy state beside an existing target is never merged', (t) => {
  const { legacy, target } = fixture(t);
  fs.mkdirSync(target);
  assert.equal(move(legacy, target).status, 3);
  assert.ok(fs.existsSync(path.join(legacy, 'jobs', 'job-1.json')));
  assert.deepEqual(fs.readdirSync(target), []);
});

test('a file held open inside legacy state defers the move', { skip: process.platform !== 'win32' }, async (t) => {
  const { legacy, target } = fixture(t);
  const holder = spawn(process.execPath, ['-e', 'require("fs").openSync(process.argv[1], "a"); console.log("held"); process.stdin.resume()', path.join(legacy, 'sccache-server.log')], { stdio: ['pipe', 'pipe', 'inherit'] });
  t.after(() => holder.kill());
  await new Promise((resolve) => holder.stdout.once('data', resolve));
  assert.equal(move(legacy, target).status, 75);
  assert.ok(fs.existsSync(path.join(legacy, 'jobs', 'job-1.json')));
  assert.ok(!fs.existsSync(target));
});
