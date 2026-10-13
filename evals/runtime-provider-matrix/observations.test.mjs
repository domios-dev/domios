import { test } from 'node:test';
import assert from 'node:assert/strict';
import { taskStatus, restoredOnNewBuild } from './observations.mjs';

test('durable ACK is read from the CLI data envelope', () => {
  assert.equal(taskStatus({ data: { task: { status: 'in_progress' } } }), 'in_progress');
  assert.equal(taskStatus({ task: { status: 'pending' } }), 'pending');
});

test('new build restores exact loaded chat thread even when its nonce persists', () => {
  const fixture = { build: 'old-build', session: 'owned-thread', nonce: 'persisted' };
  const health = { build_sha: 'new-build' };
  const tab = {
    agent_session_id: 'owned-thread', session_binding_nonce: 'persisted',
    backing: { kind: 'chat_backend' }, agent_runtime_state: { state: 'loaded' },
  };
  assert.equal(restoredOnNewBuild(fixture, health, tab), true);
  assert.equal(restoredOnNewBuild(fixture, { build_sha: 'old-build' }, tab), false);
  assert.equal(restoredOnNewBuild(fixture, health, { ...tab, agent_session_id: 'foreign-thread' }), false);
  assert.equal(restoredOnNewBuild(fixture, health, { ...tab, agent_runtime_state: { state: 'unloaded' } }), false);
});

test('compaction uses the transcript marker, not prose or type substrings', async () => {
  const { isCompactionItem } = await import('./observations.mjs');
  assert.equal(isCompactionItem({ type: 'system', timelineMarker: { kind: 'context_compaction' } }), true);
  assert.equal(isCompactionItem({ type: 'system', systemSubtype: 'contextCompaction' }), true);
  assert.equal(isCompactionItem({ type: 'assistant', content: 'context compacted' }), false);
  assert.equal(isCompactionItem({ type: 'user', content: 'context compacted' }), false);
});

test('assistant reply is reread after idle and excludes stale answers and user echoes', async () => {
  const { completedAssistantReply } = await import('./observations.mjs');
  const stale = { id: 'prior', type: 'assistant', content: 'old answer' };
  const echoed = { id: 'echo', type: 'user', content: 'expected role' };
  const preview = { id: 'new', type: 'assistant', content: 'first fragment' };
  const final = { ...preview, content: 'final assigned role' };
  let sent = false, idle = false;
  const result = await completedAssistantReply({
    history: async () => idle ? [stale, echoed, final] : [stale],
    send: async () => { sent = true; },
    waitForReply: async matches => {
      assert.equal(sent, true);
      assert.equal(matches([stale, echoed]), false);
      assert.equal(matches([stale, echoed, preview]), true);
    },
    waitIdle: async () => { idle = true; },
  });
  assert.equal(result, 'final assigned role');
});
