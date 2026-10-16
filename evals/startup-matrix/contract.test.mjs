import test from 'node:test';
import assert from 'node:assert/strict';
import { replyProven } from './contract.mjs';

import { resolveDevCli, cliToReceivedMs, devCli } from './phase-cli.mjs';
import { mkdtempSync, mkdirSync, writeFileSync, chmodSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('reply evidence requires the current token in the current own-session turn window', () => {
  const tab = { marker: 'new-token' };
  const proof = {
    marker: tab.marker, prompt: 'new-prompt', baseline_ids: [], session_before: 'own-session',
    record: { agent_session_id: 'own-session', agent_state: { state: 'idle' } },
    chat: { messages: [
      { id: 'user', type: 'user', content: 'new-prompt' },
      { id: 'old', type: 'assistant', content: 'old-token' },
    ] },
  };
  assert.equal(replyProven(tab, proof), false);
  proof.chat.messages.push({ id: 'new', type: 'assistant', content: tab.marker });
  assert.equal(replyProven(tab, proof), true);
  assert.equal(replyProven(tab, { ...proof, marker: 'old-token' }), false);
  assert.equal(replyProven(tab, { ...proof, record: { ...proof.record, agent_session_id: 'foreign-session' } }), false);
  assert.equal(replyProven(tab, { ...proof, baseline_ids: ['new'] }), false);
});


test('CLI timing uses only the pinned dev executable and strips caller binding', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'thin-cli-'));
  try {
    mkdirSync(join(directory, 'bin'));
    const executable = join(directory, 'bin', process.platform === 'win32' ? 'codeterm.exe' : 'codeterm');
    writeFileSync(executable, ''); chmodSync(executable, 0o755);
    const env = { CODETERM_DEV_DATA_DIR: directory, CODETERM_API_PORT: '7686',
      CODETERM_API_URL: 'http://127.0.0.1:7686', CODETERM_API_TOKEN: 'test-token', CODETERM_SOCKET_PATH: 'codeterm-dev.sock',
      CODETERM_PLAN_ID: 'foreign-plan', CODETERM_TASK_ID: 'foreign-task', CODETERM_SESSION_BINDING_NONCE: 'foreign-nonce' };
    const connection = resolveDevCli(env);
    assert.equal(connection.executable, executable);
    if (process.platform === 'win32') {
      const msysDirectory = directory.replace(/^([a-z]):[\\/]/i, (_, drive) => `/${drive.toLowerCase()}/`).replaceAll('\\', '/');
      assert.equal(resolveDevCli({ ...env, CODETERM_DEV_DATA_DIR: msysDirectory }).executable.toLowerCase(), executable.toLowerCase());
    }
    assert.equal(connection.env.CODETERM_DATA_DIR, directory);
    assert.equal(connection.env.CODETERM_TAB_ID, '');
    for (const key of ['CODETERM_PLAN_ID', 'CODETERM_TASK_ID', 'CODETERM_SESSION_BINDING_NONCE']) assert.equal(connection.env[key], undefined);
    for (const override of [{ CODETERM_API_PORT: '7685' }, { CODETERM_SOCKET_PATH: 'codeterm-123.sock' }, { CODETERM_API_TOKEN: '' }]) {
      assert.throws(() => resolveDevCli({ ...env, ...override }));
    }
    let began;
    const result = await devCli(directory, { executable: process.execPath, env: connection.env },
      ['-e', 'process.stdout.write(JSON.stringify({data:{ok:true}}))', '--'], AbortSignal.timeout(5000), at => { began = at; });
    assert.deepEqual(result, { ok: true });
    assert.equal(typeof began, 'number');
    assert.equal(cliToReceivedMs(began, began * 1000 + 65250), 65.25);
    assert.equal(cliToReceivedMs(began, undefined), null);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
