// WHAT: The upgrade bench decodes current CLI result envelopes.
// WHY: Bare-payload assumptions refused a genuine named instance before import.
// ORACLE: Injected command outputs preserve each payload, while errors stop before import.
import assert from 'node:assert/strict';
import test from 'node:test';
import {cliResult, decodeCliResult} from './cli-result.mjs';

test('current identify/providers/chat/inspect/import envelopes preserve their payload shapes', async () => {
  const examples = [
    {args: ['identify', '--json'], payload: {connection: {instance: 'upgrade-restore-fixture'}}},
    {args: ['agent', 'providers', '--json'], payload: [{id: 'fixture', cli_status: {resolved_path: '/owned/bin/fixture'}}]},
    {args: ['tab', 'chat', '--tab', 'own', '--json'], payload: {messages: [{id: 'message', type: 'user', content: 'opaque'}]}},
    {args: ['tab', 'inspect', '--tab', 'own', '--json'], payload: [{id: 'own', agent_session_id: 'own-session'}]},
    {args: ['sessions', 'backups', 'import', '--file', 'owned.json', '--json'], payload: {imported_tabs: 3, restart_required: true, boot_id: 'boot'}},
  ];
  for (const {args, payload} of examples) {
    let calls = 0;
    const value = await cliResult(async (...received) => {
      calls++; assert.deepEqual(received, args);
      return {stdout: JSON.stringify({result: payload})};
    }, ...args);
    assert.deepEqual(value, payload);
    assert.equal(calls, 1);
  }
});

test('scalar and object results are unwrapped once with current envelope metadata', () => {
  for (const payload of ['opaque', 42, true, false, {result: 'provider-owned-field'}, []]) {
    assert.deepEqual(decodeCliResult(JSON.stringify({result: payload, warnings: [], meta: {count: 1}, next: null})), payload);
  }
});

test('missing, ambiguous, error and legacy bare payloads refuse before an import spy', async () => {
  const refused = [
    {}, {connection: {instance: 'upgrade-restore-fixture'}}, [{id: 'fixture'}], 'bare', null,
    {result: null}, {result: [], data: []}, {result: [], providers: []},
    {ok: false, result: {connection: {instance: 'upgrade-restore-fixture'}}},
    {result: {ok: false, reason: 'refused'}}, {error: 'refused', result: {}}, {result: {error: 'refused'}},
  ];
  for (const envelope of refused) {
    let imports = 0;
    await assert.rejects(async () => {
      await cliResult(async () => ({stdout: JSON.stringify(envelope)}), 'identify', '--json');
      imports++;
    }, /CLI result/);
    assert.equal(imports, 0);
  }
  assert.throws(() => decodeCliResult('not json'), SyntaxError);
  await assert.rejects(cliResult(async () => { throw new Error('Process exited 1'); }), /Process exited 1/);
});
