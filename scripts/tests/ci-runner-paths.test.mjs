import assert from 'node:assert/strict';
import test from 'node:test';
import { runnerToolPaths } from '../ci-runner-paths.mjs';

const has = (...files) => (file) => files.includes(file);

test('Windows service Git cmd path resolves its sibling Bash bin before Rust setup', () => {
  const paths = runnerToolPaths('win32', { PATH: 'D:\\Git\\cmd;C:\\Windows\\system32', USERPROFILE: 'C:\\Users\\runner' }, has('D:\\Git\\bin\\bash.exe', 'C:\\Users\\runner\\.cargo\\bin\\rustup.exe'));
  assert.deepEqual(paths, ['D:\\Git\\bin', 'C:\\Users\\runner\\.cargo\\bin']);
});
test('standard Git install with spaces and durable Rust proxies resolve without global PATH edits', () => {
  assert.deepEqual(runnerToolPaths('win32', { ProgramFiles: 'C:\\Program Files', USERPROFILE: 'C:\\Users\\runner' }, has('C:\\Program Files\\Git\\bin\\bash.exe', 'C:\\Users\\runner\\tools\\rustup-proxies\\rustup.exe')), ['C:\\Program Files\\Git\\bin', 'C:\\Users\\runner\\tools\\rustup-proxies']);
});
test('missing Windows Bash refuses before composite actions', () => {
  assert.throws(() => runnerToolPaths('win32', {}, has()), /Git Bash is required/);
});
test('already visible Bash and duplicate Rust homes do not duplicate directories', () => {
  assert.deepEqual(runnerToolPaths('win32', { PATH: '"D:\\Git\\bin"', CARGO_HOME: 'D:\\Cargo', USERPROFILE: 'D:\\Runner' }, has('D:\\Git\\bin\\bash.exe', 'D:\\Cargo\\bin\\rustup.exe')), ['D:\\Git\\bin', 'D:\\Cargo\\bin']);
});
test('Unix runners recover their own Rust path without looking for Windows Bash', () => {
  assert.deepEqual(runnerToolPaths('linux', { HOME: '/home/runner', CARGO_HOME: '/home/runner/.cargo' }, has('/home/runner/.cargo/bin/rustup')), ['/home/runner/.cargo/bin']);
  assert.deepEqual(runnerToolPaths('darwin', { HOME: '/Users/runner' }, has()), []);
});
test('job-local path protocol rejects embedded line breaks', () => {
  assert.throws(() => runnerToolPaths('linux', { HOME: '/home/runner\nextra' }, () => true), /line break/);
});

test('Windows service PATH discovers a user-installed Python without a fixed version', () => {
  const root = 'D:\\Profile\\AppData\\Local\\Programs\\Python';
  const python = `${root}\\Python314`;
  const env = { PATH: 'D:\\Git\\bin', LOCALAPPDATA: 'D:\\Profile\\AppData\\Local' };
  const files = has('D:\\Git\\bin\\bash.exe', `${python}\\python.exe`);
  assert.deepEqual(runnerToolPaths('win32', env, files, directory => directory === root ? ['Python311', 'Python314', 'unrelated'] : []), ['D:\\Git\\bin', python]);
});

test('Windows Python discovery preserves a real interpreter already on PATH', () => {
  assert.deepEqual(runnerToolPaths('win32', { PATH: 'D:\\Git\\bin;D:\\Python' },
    has('D:\\Git\\bin\\bash.exe', 'D:\\Python\\python.exe')), ['D:\\Git\\bin', 'D:\\Python']);
});

test('Windows Store aliases and directories without an interpreter are not Python runtimes', () => {
  const store = 'D:\\Profile\\AppData\\Local\\Microsoft\\WindowsApps';
  const env = { PATH: `D:\\Git\\bin;${store}`, LOCALAPPDATA: 'D:\\Profile\\AppData\\Local' };
  assert.deepEqual(runnerToolPaths('win32', env, has('D:\\Git\\bin\\bash.exe', `${store}\\python.exe`), () => ['Python399']), ['D:\\Git\\bin']);
});
import { checkRunnerTools, RUNNER_ROLES, runnerPathContent } from '../ci-runner-paths.mjs';

test('role preflight checks every required tool and reports all missing prerequisites before compilation', () => {
  for (const role of Object.keys(RUNNER_ROLES)) {
    const commands = [];
    checkRunnerTools(role, { platform: 'linux', execute: (command, args, options) => {
      commands.push(command);
      assert.ok(options.timeout <= 2000);
      assert.deepEqual(args, ['--version']);
      return { status: 0 };
    } });
    assert.equal(commands.length, RUNNER_ROLES[role].length);
    assert.throws(() => checkRunnerTools(role, { execute: () => ({ status: 1 }) }),
      error => RUNNER_ROLES[role].every(tool => error.message.includes(tool)) && error.message.includes('build-machine/SKILL.md'));
  }
});

test('preflight rejects missing Python even when its path directory was discovered', () => {
  assert.throws(() => checkRunnerTools('rust', { platform: 'win32', execute: command =>
    ({ status: command === 'python' ? 9009 : 0 }) }), /python/);
  assert.throws(() => checkRunnerTools('unknown'), /Unknown runner role/);
});

test('GitHub job-local path entries are separate physical lines', () => {
  assert.equal(runnerPathContent(['C:/Program Files/Git/bin', 'C:/Python']), 'C:/Program Files/Git/bin\nC:/Python\n');
  assert.equal(runnerPathContent([]), '');
});
