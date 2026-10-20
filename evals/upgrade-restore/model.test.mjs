import {test} from 'node:test';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
import {worstCases} from './fixtures.mjs';
import {leaves, validateCapture, requireBackings, verdict, failureReason, measurements, requireRestart} from './model.mjs';
import {peer} from './fake-peer.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const fixture = () => worstCases(root, '/isolated-fixture-project');
function ready(tab) {
  return {...tab, agent_session_id: tab.claude_session_id, task_binding: tab.agent_identity?.task_binding,
    session_binding_nonce: 'nonce-current', reported_session_binding_nonce: 'nonce-current',
    agent_runtime_state: {state: 'loaded', epoch: 1}};
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
  document.histories[2].messages.pop();
  assert.throws(() => validateCapture(document), /history_bytes changed/);
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
});

test('fake app-server refuses unknown threads and unsupported mutations', () => {
  const respond = peer(fixture());
  assert.throws(() => respond({method: 'thread/resume', params: {threadId: 'foreign'}}), /no thread/);
  assert.throws(() => respond({method: 'turn/start', params: {threadId: 'thread-fixture-general'}}), /Unsupported/);
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

test('shell metadata can be captured while replay refuses its missing fake backing', () => {
  const document = fixture();
  document.snapshot.workspaces[0].tabs[2].split_tree.agent_type = null;
  document.measurements = measurements(document.snapshot, document.histories);
  assert.equal(validateCapture(document).length, 6);
  assert.throws(() => requireBackings(document), /fixture-long-history.*fake shell\/surface backing missing/);
});

test('readiness requires an actual daemon restart and the exact staged fixture', () => {
  const stage = {boot_id: 'before', fixture_sha256: 'fixture'};
  assert.throws(() => requireRestart(stage, 'before', 'fixture'), /has not restarted/);
  assert.throws(() => requireRestart({}, 'after', 'fixture'), /has not restarted/);
  assert.throws(() => requireRestart(stage, 'after', 'different'), /differs from the staged/);
  assert.doesNotThrow(() => requireRestart(stage, 'after', 'fixture'));
});
