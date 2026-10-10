import assert from 'node:assert/strict';
import path from 'node:path';
export const providers = ['claude', 'codex', 'opencode', 'grok'];
export const checks = ['peer-switch','gateway-return','tab-list','chat-render','composer-send','stop',
  'model-switch','managed-by','status-working','status-done','progress-live','sync-within-1s',
  'tool-rows','code-blocks','attachments','history-reload','medium-chat-15-30'];
export const directions = ['mac-web-to-windows','windows-web-to-mac'];
export function cheap(provider, id) {
  return typeof id === 'string' && ({claude: /^(?:haiku|claude-haiku-[a-zA-Z0-9.-]+)$/, codex: /^gpt-6-luna$/,
    opencode: /(^|\/)deepseek(?:-flash)?$/, grok: /^grok-4\.6$/}[provider])?.test(id);
}
export function validateManifest(m) {
  assert.equal(m.rcRoot, '/Users/dev/Developer/codeterm-worktrees/rc-mac');
  assert.equal(m.mac.origin, 'http://100.64.10.2:7697');
  assert.equal(m.windows.origin, 'http://100.64.10.1:7686');
  assert.ok(m.mac.peerId !== m.windows.peerId);
  for(const machine of [m.mac,m.windows]) assert.match(machine.peerId,/^[a-fA-F0-9-]{6,}$/);
  for (const machine of [m.mac, m.windows]) {
    assert.ok(machine.tokenFile);
    assert.equal(machine.tabs.length, 4);
    assert.deepEqual([...new Set(machine.tabs.map(t => t.provider))].sort(), [...providers].sort());
    for (const tab of machine.tabs) {
      assert.match(tab.tabId,/^[a-zA-Z0-9_-]+$/);
      assert.equal(tab.testOwned,true,'Only assigned test fixtures may receive traffic');
      assert.ok(tab.sessionId && tab.source.path && tab.title);
      assert.ok(cheap(tab.provider, tab.model), 'Fixture model exceeds authorized cheap family');
      if (tab.switchModel) assert.ok(cheap(tab.provider, tab.switchModel));
      assert.equal(tab.source.provider, tab.provider);
      assert.equal(tab.source.session_id, tab.sessionId);
      if (tab.provider === 'grok') assert.ok(tab.source.summary_path);
      if (machine === m.windows) {assert.ok(machine.recordShellTab && machine.python);assert.equal(machine.recordShellTestOwned,true);}
    }
  }
  return m;
}
export function claim(snapshot, tab, exactPrompt, boundary = 0, expectedModel) {
  assert.equal(snapshot.session_id, tab.sessionId);
  assert.equal(snapshot.provider, tab.provider);
  const after = snapshot.records.slice(boundary);
  const hits = after.map((r,i) => [r,i]).filter(([r]) => r.role === 'user' && r.text === exactPrompt);
  assert.equal(hits.length, 1, 'Exact submitted user turn must persist exactly once');
  const [user, index] = hits[0];
  const following = after.slice(index + 1);
  const nextUser = following.findIndex(r => r.role === 'user');
  const assistant = following.slice(0, nextUser < 0 ? undefined : nextUser).filter(r => r.role === 'assistant');
  assert.ok(assistant.some(r => r.text.trim()), 'Same turn needs persisted assistant output');
  if (expectedModel) assert.ok(assistant.some(r => r.model === expectedModel), 'Model needs same-turn provider proof');
  return {user, assistant, boundary};
}
export function matrix(rows) {
  const out = ['# Phase B2 matrix', '', 'NOT RUN means no execution evidence.', '',
    '| Provider | Check | View | Direction | Verdict | Evidence |', '|---|---|---|---|---|---|'];
  for (const provider of providers) for (const check of checks) for (const view of ['local','remote','web UI']) for (const direction of directions) {
    const matches = rows.filter(r => r.provider===provider && r.check===check && r.view===view && r.direction===direction);
    for (const r of matches.length ? matches : [{status:'NOT RUN', evidence:'No receipt'}]) {
      out.push(`| ${provider} | ${check} | ${view} | ${direction} | ${r.status} | ${r.evidence==='No receipt' ? r.evidence : '['+r.evidence+']('+r.evidence+')'} |`);
    }
  }
  return out.join('\n') + '\n';
}

export function validateOutput(out) {
  const resolved=path.resolve(out);
  assert.equal(path.dirname(resolved),'/Users/dev/Developer/codeterm-worktrees/rc-mac/target-check/evals');
  assert.match(path.basename(resolved),/^mesh-(?:webui|terminals)-[a-zA-Z0-9_.-]+$/);
  return resolved;
}
export function convergence(turn) {
  return ['working','idle'].map(state=>{
    const web=state==='idle' ? [...turn.web].reverse().find(e=>e.state===state) : turn.web.find(e=>e.state===state);
    const home=state==='idle' ? [...turn.home].reverse().find(e=>e.state===state) : turn.home.find(e=>e.state===state);
    assert.ok(web&&home,'Home-web and switched-web need transition receipts');
    assert.ok(Math.abs(web.at-home.at)<=1000,'View convergence exceeds one second');
    return {state,web:web.at,home:home.at};
  });
}
