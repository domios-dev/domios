import test from 'node:test';
import assert from 'node:assert/strict';
import { replyProven } from './contract.mjs';

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
