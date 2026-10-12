import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ownedSidecar, ownedGroups } from '../dev-sidecar-ownership.mjs';

const root = '/home/user/.codeterm-dev-aiderqa';
const command = token => `node /bin/codex app-server --ws-token-file ${token}`;

test('only exact dev instance token ownership permits reaping', () => {
  assert.ok(ownedSidecar(command(`${root}/codex-rpc/a.token`), root));
  for (const token of ['/home/user/.codeterm/codex-rpc/a.token', '/home/user/.codeterm-dev/codex-rpc/a.token', `${root}-other/codex-rpc/a.token`, `${root}/codex-rpc/../a.token`]) {
    assert.equal(ownedSidecar(command(token), root), false);
  }
  assert.equal(ownedSidecar(command('/home/user/.codeterm/codex-rpc/a.token'), '/home/user/.codeterm'), false);
  assert.equal(ownedSidecar(`cat --ws-token-file ${root}/codex-rpc/a.token`, root), false);
});

test('quoted paths and Windows separators preserve instance boundaries', () => {
  const winRoot = 'C:\\Users\\User Name\\.codeterm-dev-qa';
  assert.ok(ownedSidecar(command(`"${winRoot}\\codex-rpc\\a.token"`), winRoot, 'win32'));
  assert.equal(ownedSidecar(command('"C:\\Users\\User Name\\.codeterm\\codex-rpc\\a.token"'), winRoot, 'win32'), false);
  assert.ok(ownedSidecar(command(`"/home/user name/.codeterm-dev/codex-rpc/a.token"`), '/home/user name/.codeterm-dev'));
});

test('Unix cleanup requires an owned group leader, never an inherited group', () => {
  const cmd = command(`${root}/codex-rpc/a.token`);
  assert.deepEqual(ownedGroups([{ pid: 10, group: 10, command: cmd }, { pid: 11, group: 10, command: cmd }, { pid: 1, group: 1, command: cmd }], root).map(p => p.pid), [10]);
});
