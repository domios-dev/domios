import {test} from 'node:test';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
import {worstCases} from './fixtures.mjs';
import {leaves, validateCapture, requireBackings, verdict, failureReason, measurements, requireRestart, historyMatches,
  synthesizeHistory} from './model.mjs';
import {peer} from './fake-peer.mjs';
import {unavailableVerdict, failedLaunchVerdict} from './unavailable.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const fixture = () => worstCases(root, '/isolated-fixture-project');
function ready(tab) {
  return {...tab, task_binding: tab.agent_identity?.task_binding,
    session_binding_nonce: 'nonce-current', reported_session_binding_nonce: 'nonce-current',
    agent_runtime_state: {state: 'loaded', epoch: 1, history: 'complete', historyPages: 1}};
}

test('capture verifies all leaf, prompt, identity and normalized history byte sizes', () => {
  const document = fixture();
  assert.equal(validateCapture(document).length, 6);
  assert.equal(requireBackings(document).length, 6);
});

test('capture rejects unmasked prompt content even with a matching length', () => {
  const document = fixture();
  const config = document.snapshot.workspaces[0].tabs[0].split_tree.agent_identity.agent_runtime_config;
  config.systemPrompt = 's' + config.systemPrompt.slice(1);
  assert.throws(() => validateCapture(document), /unmasked content/);
});

test('capture rejects silently truncated history', () => {
  const document = fixture();
  document.histories[2].shape.pop();
  assert.throws(() => validateCapture(document), /history_items changed/);
});

test('capture refuses v1 documents and any transcript text in the history shape', () => {
  const v1 = fixture();
  v1.format = 'upgrade-restore-v1';
  assert.throws(() => validateCapture(v1), /upgrade-restore-v2/);
  const messages = fixture();
  messages.histories[2].messages = synthesizeHistory(messages.histories[2].shape);
  assert.throws(() => validateCapture(messages), /without transcript content/);
  const text = fixture();
  text.histories[2].shape[0].content = 'private';
  assert.throws(() => validateCapture(text), /malformed history shape row 0/);
  const group = fixture();
  group.histories[2].shape[0].group = 1;
  assert.throws(() => validateCapture(group), /malformed history shape row 0/);
});

test('capture refuses history volume smaller than its shape payload', () => {
  const document = fixture();
  document.measurements[2].history_bytes = 4096;
  assert.throws(() => validateCapture(document), /history_bytes is smaller/);
});

test('prepare keeps the captured transcript volume while leaf sizes follow the rewritten desk', () => {
  const document = fixture();
  document.measurements[2].history_bytes = 50 * 1024 * 1024;
  leaves(document)[2].cwd = '/prepared/project';
  const prepared = measurements(document.snapshot, document.histories, document.measurements);
  assert.equal(prepared[2].history_bytes, 50 * 1024 * 1024);
  assert.equal(prepared[2].leaf_bytes, Buffer.byteLength(JSON.stringify(leaves(document)[2])));
  assert.equal(validateCapture({...document, measurements: prepared}).length, 6);
});

test('capture keeps unavailable transcripts explicit and replay refuses missing evidence', () => {
  const document = fixture();
  const history = document.histories[0];
  history.shape = [];
  history.transcript = {status: 'unavailable', backing_kind: 'chat_backend', provider: 'fixture-provider'};
  document.measurements = measurements(document.snapshot, document.histories);
  assert.equal(validateCapture(document).length, 6);
  assert.throws(() => requireBackings(document), /transcript unavailable \(backing=chat_backend, provider=fixture-provider\)/);
});

test('captured unavailable state has a separate passive oracle and rejects activation or identity drift', () => {
  const document = fixture();
  const tab = leaves(document)[1];
  const state = {state: 'degraded', epoch: 3, reason: 'opaque refusal'};
  tab.agent_identity.agent_runtime_state = state;
  document.histories[1].transcript = {status: 'unavailable', backing_kind: 'chat_backend', provider: 'codex'};
  document.measurements = measurements(document.snapshot, document.histories);
  assert.equal(requireBackings(document).length, 6);
  const actual = {...tab, agent_session_id: tab.agent_session_id,
    backing: {kind: 'pty'},
    session_binding_nonce: tab.agent_identity.session_binding_nonce,
    agent_runtime_state: structuredClone(state), frozen: true, pty_id: null};
  assert.deepEqual(unavailableVerdict(document, tab, actual, []), {
    tab_id: tab.tab_id, bucket: 'preserved-unavailable', preserved: true,
    reason: 'captured unavailable state and identity preserved without activation'});
  assert.equal(verdict(tab, actual).ready, false);
  for (const changed of [{...actual, agent_session_id: 'other'}, {...actual, frozen: false},
    {...actual, agent_runtime_state: {...state, epoch: 4}}, {...actual, pty_id: 'unauthorized'},
    {...actual, session_binding_nonce: 'new-launch'}, {...actual, backing: {kind: 'surface'}}]) {
    assert.equal(unavailableVerdict(document, tab, changed, []).preserved, false);
  }
  assert.equal(unavailableVerdict(document, tab, actual, [{thread_id: tab.agent_session_id}]).preserved, false);
});

test('failed role-bearing PTY conserves failure and hierarchy without utility or runtime authority', () => {
  const document = fixture();
  const tab = leaves(document)[0];
  tab.agent_type = null;
  delete tab.agent_session_id;
  delete tab.claude_session_id;
  delete tab.agent_identity.agent_runtime_state;
  delete tab.agent_identity.session_binding_nonce;
  delete tab.agent_identity.reported_session_binding_nonce;
  delete tab.agent_identity.task_binding;
  tab.agent_identity.boot_stage = 'spawn_failed';
  tab.agent_identity.boot_failure_reason = 'xxxxxxxx: xxxxxxxx';
  const history = document.histories[0];
  history.shape = [];
  history.transcript = {status: 'failed_launch', backing_kind: 'pty', provider: null,
    failed_launch: {boot_stage: 'spawn_failed', boot_failure_reason: 'xxxxxxxx: xxxxxxxx'}};
  document.measurements = measurements(document.snapshot, document.histories);
  assert.equal(requireBackings(document).length, 6);
  const actual = {...tab, ...tab.agent_identity, backing: {kind: 'pty'}, frozen: true,
    pty_id: null, agent_booting: false, agent_state: null, agent_runtime_state: null};
  assert.deepEqual(failedLaunchVerdict(document, tab, actual, []), {
    tab_id: tab.tab_id, bucket: 'preserved-failed-launch', preserved: true,
    reason: 'captured launch failure and hierarchy preserved without activation'});
  assert.equal(unavailableVerdict(document, tab, actual, []).preserved, false);
  assert.equal(verdict(tab, actual).ready, false);
  for (const changed of [{...actual, role: null}, {...actual, boot_failure_reason: null},
    {...actual, boot_stage: 'shell_ready'}, {...actual, agent_type: 'claude'},
    {...actual, agent_session_id: 'invented'}, {...actual, reported_session_binding_nonce: 'invented'},
    {...actual, frozen: false}, {...actual, pty_id: 'activated'},
    {...actual, backing: {kind: 'surface'}}, {...actual, agent_runtime_state: {state: 'loaded'}}]) {
    assert.equal(failedLaunchVerdict(document, tab, changed, []).preserved, false);
  }
  assert.equal(failedLaunchVerdict(document, tab, actual, [{tab_id: tab.tab_id}]).preserved, false);
  history.transcript.failed_launch.boot_failure_reason = 'changed';
  assert.throws(() => requireBackings(document), /failed launch evidence changed/);
  history.transcript = {status: 'not_applicable', backing_kind: 'pty', provider: null};
  assert.throws(() => requireBackings(document), /unsupported utility backing evidence/);
});

test('capture refuses missing plan backing for a legacy plan/task-only Worker', () => {
  const document = fixture();
  document.plans = [];
  assert.throws(() => requireBackings(document), /fixture-legacy-worker.*binding incomplete/);
});

test('capture refuses a retired execution attempt', () => {
  const document = fixture();
  document.plans[0].tasks[0].execution_attempts[0].retired_at = 3;
  assert.throws(() => requireBackings(document), /fixture-legacy-worker.*0 matching live attempts/);
});

test('capture refuses ambiguous live execution attempts', () => {
  const document = fixture();
  document.plans[0].tasks[0].execution_attempts.push({...document.plans[0].tasks[0].execution_attempts[0], id: 'other'});
  assert.throws(() => requireBackings(document), /2 matching live attempts/);
});

test('capture fails closed for a hidden tab absent from persisted replay backing', () => {
  const document = fixture();
  document.live_tab_metadata.push({tab_id: 'missing-hidden', hidden: true, show_in_tabbar: false});
  assert.throws(() => requireBackings(document), /missing-hidden.*no persisted replay backing/);
});

test('fixture duplicates full manual-size identity and prompt above the desk leaf bound', () => {
  const document = fixture();
  for (const id of ['fixture-orchestrator', 'fixture-general']) {
    const measure = document.measurements.find(row => row.tab_id === id);
    assert.ok(measure.prompt_bytes >= 48 * 1024);
    assert.ok(measure.identity_bytes >= 48 * 1024);
    assert.ok(measure.leaf_bytes > 64 * 1024);
  }
});

test('fake app-server sends real Codex page shapes and more than 1 MiB of history', () => {
  const document = fixture();
  const respond = peer(document);
  const threadId = 'thread-fixture-long-history';
  const turns = respond({method: 'thread/turns/list', params: {threadId}});
  const first = respond({method: 'thread/items/list', params: {threadId, turnId: turns.data[0].id, limit: 100}});
  assert.equal(first.data[0].type, 'agentMessage');
  assert.ok(Buffer.byteLength(JSON.stringify(first)) > 256 * 1024);
  let cursor;
  let bytes = 0;
  let count = 0;
  do {
    const page = respond({method: 'thread/items/list', params: {threadId, turnId: turns.data[0].id, cursor}});
    bytes += page.data.reduce((sum, row) => sum + Buffer.byteLength(row.text ?? row.content?.find(part => part.type === 'text')?.text ?? ''), 0);
    count += page.data.length;
    cursor = page.nextCursor;
  } while (cursor);
  assert.equal(count, 320);
  assert.ok(bytes > 1024 * 1024);
  const last = respond({method: 'thread/items/list', params: {threadId, turnId: turns.data[0].id, cursor: '300'}});
  assert.equal(last.data.at(-1).content[1].type, 'image');
  assert.equal(document.measurements[2].image_bytes, 16384);
  assert.deepEqual(respond({method: 'thread/items/list', params: {threadId, turnId: turns.data[0].id, cursor: '300'}}), last);
  assert.equal(last.data.at(-1).id, synthesizeHistory(document.histories[2].shape).at(-1).id);
  assert.ok(Buffer.byteLength(JSON.stringify(document.histories)) < 32 * 1024);
});

test('fake app-server groups synthesized items by captured tool-flow group and keeps tool payload sizes', () => {
  const document = fixture();
  document.histories[2].shape = [{kind: 'assistant', group: 0, bytes: 3},
    {kind: 'tool_call', group: 1, bytes: 2, result_bytes: 5, input_bytes: 40}, {kind: 'assistant', group: 0, bytes: 1}];
  document.measurements = measurements(document.snapshot, document.histories);
  assert.equal(validateCapture(document).length, 6);
  const respond = peer(document);
  const threadId = 'thread-fixture-long-history';
  const turns = respond({method: 'thread/turns/list', params: {threadId}}).data;
  assert.deepEqual(turns.map(turn => turn.id), ['turn-0', 'turn-1']);
  const first = respond({method: 'thread/items/list', params: {threadId, turnId: 'turn-0'}}).data;
  assert.deepEqual(first.map(item => item.id), ['message-0', 'message-2']);
  const [call] = respond({method: 'thread/items/list', params: {threadId, turnId: 'turn-1'}}).data;
  assert.equal(call.result, 'xxxxx');
  assert.equal(Buffer.byteLength(JSON.stringify(call.arguments)), 40);
});

test('fake app-server refuses unknown threads and unsupported mutations', () => {
  const respond = peer(fixture());
  assert.throws(() => respond({method: 'thread/resume', params: {threadId: 'foreign'}}), /no thread/);
  assert.throws(() => respond({method: 'turn/start', params: {threadId: 'thread-fixture-general'}}), /Unsupported/);
});

test('restored image history requires the exact bounded marker and retained attachment', () => {
  const expected = synthesizeHistory(fixture().histories[2].shape).at(-1);
  const actual = {id: 'message-319', type: 'user',
    content: 'x'.repeat(4096) + '\n[image omitted: 16406 bytes]',
    attachments: [{id: 'message-319-image-1', mimeType: 'image/png'}]};
  assert.equal(historyMatches(expected, actual), true);
  assert.equal(historyMatches(expected, {...actual, content: actual.content.replace('16406', '16384')}), false);
  assert.equal(historyMatches(expected, {...actual, attachments: []}), false);
  assert.equal(historyMatches(expected, {...actual, attachments: [{id: 'foreign', mimeType: 'image/png'}]}), false);
  const plain = {id: 'text', type: 'assistant', content: 'xx'};
  assert.equal(historyMatches(plain, {...plain}), true);
  assert.equal(historyMatches(plain, {...plain, content: 'xy'}), false);
});

test('oracle names SizeLimit without converting it to a timeout', () => {
  const tab = leaves(fixture())[2];
  const actual = ready(tab);
  actual.agent_runtime_state = {state: 'degraded', epoch: 1, reason: 'SizeLimit: plugin state exceeds 262144 bytes'};
  assert.deepEqual(verdict(tab, actual), {tab_id: tab.tab_id, ready: false, reason: actual.agent_runtime_state.reason});
});

test('oracle rejects ready-looking legacy Workers with missing attempt identity', () => {
  const tab = leaves(fixture())[3];
  assert.match(verdict(tab, ready(tab)).reason, /binding incomplete/);
  const actual = ready(tab);
  actual.task_binding = {...actual.task_binding, attempt_id: 'attempt_fixture-legacy-worker'};
  assert.equal(verdict(tab, actual).ready, true);
});

test('oracle rejects session drift and unconfirmed runtime nonce', () => {
  const tab = leaves(fixture())[2];
  assert.match(verdict(tab, {...ready(tab), agent_session_id: 'foreign'}).reason, /session binding changed/);
  assert.match(verdict(tab, {...ready(tab), reported_session_binding_nonce: 'foreign'}).reason, /nonce unconfirmed/);
});

test('oracle names a missing tab and rejects saved unloaded state', () => {
  const tab = leaves(fixture())[2];
  assert.equal(verdict(tab, null).reason, 'tab missing after restore');
  assert.equal(verdict(tab, {...ready(tab), agent_runtime_state: {state: 'loaded', epoch: 0}}).ready, false);
});

test('structured CLI failure preserves exact runtime reason from stdout', () => {
  assert.equal(failureReason({stdout: JSON.stringify({ok: false, error: {message: 'SizeLimit: state'}}), stderr: ''}), 'SizeLimit: state');
  assert.equal(failureReason({killed: true}), 'bounded replay deadline elapsed');
});

test('tab presence alone cannot pass shell or surface readiness', () => {
  const tab = {...leaves(fixture())[2], agent_type: null};
  assert.equal(verdict(tab, ready(tab)).ready, false);
});

test('utility replay refuses a provider history disguised as a shell', () => {
  const document = fixture();
  document.snapshot.workspaces[0].tabs[2].split_tree.agent_type = null;
  document.measurements = measurements(document.snapshot, document.histories);
  assert.equal(validateCapture(document).length, 6);
  assert.throws(() => requireBackings(document), /fixture-long-history.*unsupported utility backing evidence/);
});

test('readiness requires an actual daemon restart and the exact staged fixture', () => {
  const stage = {boot_id: 'before', fixture_sha256: 'fixture'};
  assert.throws(() => requireRestart(stage, 'before', 'fixture'), /has not restarted/);
  assert.throws(() => requireRestart({}, 'after', 'fixture'), /has not restarted/);
  assert.throws(() => requireRestart(stage, 'after', 'different'), /differs from the staged/);
  assert.doesNotThrow(() => requireRestart(stage, 'after', 'fixture'));
});
