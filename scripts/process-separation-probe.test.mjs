import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync, realpathSync, mkdirSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseProbeArgs, cleanEnvironment, attachRequest } from './process-separation-probe.mjs';
import { assertServerClosure, serverArtifact, workerArtifact } from './server-artifact.mjs';

test('only explicit disposable process cases are admitted', () => {
  assert.deepEqual(parseProbeArgs(['--case', 'server-only', '--instance', 'split-oracle', '--artifact-receipt', '/receipt']), { case: 'server-only', receipt: '/receipt' });
  for (const args of [[], ['--case', 'production'], ['--case', 'arbitration', '--instance', 'prod', '--artifact-receipt', '/receipt'], ['--case', 'arbitration', '--instance', 'split-oracle', '--port', '7685']]) assert.throws(() => parseProbeArgs(args));
});
test('launch environment discards inherited connection and display selectors', () => {
  const env = cleanEnvironment({ CODETERM_API_URL: 'foreign', CODETERM_API_TOKEN: 'secret', CODETERM_INSTANCE: 'prod', CODETERM_MACHINE_OWNER: 'true', DISPLAY: ':1', WAYLAND_DISPLAY: 'wayland', PATH: '/tools' });
  assert.deepEqual(env, { PATH: '/tools', CODETERM_MACHINE_OWNER: 'false' });
});
test('production closure refuses each GUI edge and an absent server root', () => {
  assertServerClosure(['domios-server v1.14.9', 'tokio v1.0']);
  for (const name of ['tauri', 'tauri-runtime-wry', 'wry', 'web-view', 'webview2-com', 'webkit2gtk', 'codeterm-host-runtime']) assert.throws(() => assertServerClosure(['domios-server v1.14.9', `${name} v2.0`]));
  assert.throws(() => assertServerClosure(['tokio v1.0']));
});
test('worker resource binds the CLI source artifact rather than a same-named GUI binary', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'domios-worker-artifact-unit-'));
  try {
    mkdirSync(path.join(root, 'cli/src'), { recursive: true });
    const source = path.join(root, 'cli/src/main.rs'); writeFileSync(source, 'fixture');
    const foreign = path.join(root, 'gui-main.rs'); writeFileSync(foreign, 'fixture');
    const executable = path.join(root, 'codeterm'); writeFileSync(executable, 'never-executed');
    const message = { reason: 'compiler-artifact', target: { name: 'codeterm', kind: ['bin'], src_path: source }, executable, profile: { test: false } };
    assert.equal(workerArtifact([message], root), realpathSync.native(executable));
    assert.throws(() => workerArtifact([{ ...message, target: { ...message.target, src_path: foreign } }], root));
    assert.throws(() => workerArtifact([message, message], root));
  } finally { rmSync(root, { recursive: true }); }
});
test('artifact selection rejects missing, duplicate and test executable evidence', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'domios-artifact-unit-'));
  try {
    const executable = path.join(root, 'artifact'); writeFileSync(executable, 'not-executed');
    const message = { reason: 'compiler-artifact', target: { name: 'domios-server', kind: ['bin'] }, executable, profile: { test: false } };
    assert.equal(serverArtifact([message]), realpathSync.native(executable));
    for (const messages of [[], [message, message], [{ ...message, profile: { test: true } }]]) assert.throws(() => serverArtifact(messages));
  } finally { rmSync(root, { recursive: true }); }
});
test('control requests bind challenge and exact intended server incarnation', () => {
  const ready = { protocol: 2, api_version: 3, build: 'artifact', identity: { data: '/data', config: '/data/config' }, epoch: 'boot', process: { pid: 7, created: 'born', executable: '/server' } };
  const first = attachRequest(ready), second = attachRequest(ready);
  assert.notEqual(first.challenge, second.challenge);
  assert.deepEqual(first.expected, { process: ready.process, epoch: ready.epoch });
  assert.equal(attachRequest(ready, { operation: 'shutdown' }).operation, 'shutdown');
});
