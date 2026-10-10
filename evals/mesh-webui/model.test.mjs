import test from 'node:test';
import {readFileSync} from 'node:fs';
import assert from 'node:assert/strict';
import {cheap,claim,matrix,validateOutput,convergence,validateManifest} from './model.mjs';
const tab={provider:'codex',sessionId:'own'};
const user={role:'user',text:'opaque supplied marker',locator:'line:2'};
const answer={role:'assistant',text:'opaque supplied marker',model:'gpt-6-luna',locator:'line:4'};
const snapshot={provider:'codex',session_id:'own',records:[user,answer]};
test('exact own-session user and assistant join',()=>assert.equal(claim(snapshot,tab,user.text,0,'gpt-6-luna').user,user));
test('cross-session claim refused',()=>assert.throws(()=>claim({...snapshot,session_id:'foreign'},tab,user.text)));
test('duplicate user turn refused',()=>assert.throws(()=>claim({...snapshot,records:[user,user,answer]},tab,user.text)));
test('later turn cannot answer earlier turn',()=>assert.throws(()=>claim({...snapshot,records:[user,{role:'user',text:'other'},answer]},tab,user.text)));
test('old records cannot prove new turn',()=>assert.throws(()=>claim(snapshot,tab,user.text,2)));
test('model label without own assistant model refused',()=>assert.throws(()=>claim({...snapshot,records:[user,{...answer,model:null}]},tab,user.text,0,'gpt-6-luna')));
test('cheap allowlist rejects unrelated names',()=>{
  assert.ok(cheap('claude','claude-haiku-4-5-20251001'));
  assert.ok(cheap('opencode','deepseek/deepseek-flash'));
  assert.ok(cheap('codex','gpt-6-luna'));
  assert.ok(cheap('grok','grok-4.6'));
  assert.ok(!cheap('claude','expensive-haiku-pretender'));
  assert.ok(!cheap('codex','gpt-6.1-sol'));
});
test('empty matrix contains no execution pass',()=>{
  const text=matrix([]);assert.equal(text.split('| NOT RUN |').length-1,408);assert.ok(!text.includes('| PASS |'));
});
test('later pass never erases first fail',()=>{
  const r={provider:'codex',check:'stop',view:'web UI',direction:'mac-web-to-windows',status:'FAIL',evidence:'first.json'};
  const text=matrix([r,{...r,status:'PASS',evidence:'second.json'}]);
  assert.ok(text.includes('| FAIL | [first.json](first.json)'));assert.ok(text.includes('| PASS | [second.json](second.json)'));
});
test('only approved direct RC run directories accepted',()=>{
  const root='/Users/dev/Developer/codeterm-worktrees/rc-mac/target-check/evals/';
  assert.equal(validateOutput(root+'mesh-terminals-x'),root+'mesh-terminals-x');
  assert.equal(validateOutput(root+'mesh-webui-x'),root+'mesh-webui-x');
  assert.throws(()=>validateOutput(root+'mesh-webui-x/nested'));
  assert.throws(()=>validateOutput('/Users/dev/Developer/codeterm/target-check/evals/mesh-webui-x'));
});
test('convergence uses completion idle rather than pre-turn idle',()=>{
  assert.throws(()=>convergence({web:[{state:'idle',at:0},{state:'working',at:10},{state:'idle',at:3000}],
    home:[{state:'idle',at:0},{state:'working',at:15},{state:'idle',at:100}]}));
});

test('manifest admits only the two authorized gateway origins',()=>{
  const m=JSON.parse(readFileSync(new URL('./manifest.example.json',import.meta.url),'utf8'));
  m.mac.peerId='abcdef01';m.windows.peerId='abcdef02';
  assert.equal(validateManifest(m),m);
  for(const [machine,origin] of [['windows','http://100.64.10.1:7698'],
    ['windows','http://100.64.10.1:7685'],['mac','http://100.64.10.2:7685'],
    ['windows','http://127.0.0.1:7686']]) {
    const other=structuredClone(m);other[machine].origin=origin;
    assert.throws(()=>validateManifest(other));
  }
});

test('manifest refuses gateway token files and missing secret names',()=>{
  const m=JSON.parse(readFileSync(new URL('./manifest.example.json',import.meta.url),'utf8'));
  m.mac.peerId='abcdef01';m.windows.peerId='abcdef02';
  for(const key of ['mac','windows']) {
    const missing=structuredClone(m);delete missing[key].tokenSecret;
    assert.throws(()=>validateManifest(missing));
    const plaintext=structuredClone(m);plaintext[key].tokenFile='/opaque-fixture-file';
    assert.throws(()=>validateManifest(plaintext));
  }
});

test('manifest validates optional fixture workspace identities before UI selection', () => {
  const m = JSON.parse(readFileSync(new URL('./manifest.example.json', import.meta.url), 'utf8'));
  m.mac.peerId = 'abcdef01'; m.windows.peerId = 'abcdef02';
  m.mac.tabs[0].workspaceId = '05799f42';
  assert.equal(validateManifest(m), m);
  delete m.mac.tabs[0].workspaceId;
  assert.equal(validateManifest(m), m);
  for (const workspaceId of ['', ' ', null, 42, 'bad"selector', 'mesh:peer/workspace']) {
    const other = structuredClone(m);
    other.mac.tabs[0].workspaceId = workspaceId;
    assert.throws(() => validateManifest(other), /workspaceId/);
  }
  for (const kind of ['shells', 'terminals']) {
    const other = structuredClone(m);
    other.windows[kind] = [{ tabId: 'shell-fixture', workspaceId: '' }];
    assert.throws(() => validateManifest(other), /workspaceId/);
  }
});
