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
