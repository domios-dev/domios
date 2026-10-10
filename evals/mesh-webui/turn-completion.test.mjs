import test from 'node:test';
import assert from 'node:assert/strict';
import {completeTurn,runScopedIdentity} from './turn-completion.mjs';

const tab={sessionId:'owned-session',provider:'fixture'};
const token='opaque-marker';
const prompt=`${token} opaque payload`;
const user={role:'user',text:prompt,locator:'user-1'};
const reply={role:'assistant',text:token,locator:'reply-1'};
function inputs({records=[user,reply],samples=[{state:'idle'}],sessionId=tab.sessionId,boundary=0}={}) {
  return {tab,prompt,token,boundary,waitReply:async()=>{},
    snapshot:async()=>({proof:{session_id:sessionId,provider:tab.provider,records},evidence:'native.json'}),
    readActivity:async()=>samples};
}

test('own-session reply confirms a turn independently of missing Working',async()=>{
  const result=await completeTurn(inputs());
  assert.equal(result.receipt.user.locator,'user-1');
  assert.equal(result.working.status,'FAIL');
  assert.equal(result.after.evidence,'native.json');
});
test('reply boundary precedes native proof and Working classification',async()=>{
  const events=[];
  const config=inputs({samples:[{state:'working'},{state:'idle'}]});
  const snapshot=config.snapshot;
  config.waitReply=async()=>events.push('reply');
  config.snapshot=async()=>{events.push('native');return snapshot();};
  config.readActivity=async()=>{events.push('activity');return [{state:'working'}];};
  const result=await completeTurn(config);
  assert.deepEqual(events,['reply','native','activity']);
  assert.equal(result.working.status,'PASS');
});
test('Working alone cannot confirm an unreplied turn',async()=>{
  await assert.rejects(completeTurn(inputs({records:[user],samples:[{state:'working'}]})));
});
test('rendered marker alone cannot confirm a missing user record',async()=>{
  await assert.rejects(completeTurn(inputs({records:[reply]})));
});
test('another session or pre-boundary history cannot confirm delivery',async()=>{
  await assert.rejects(completeTurn(inputs({sessionId:'foreign-session'})));
  await assert.rejects(completeTurn(inputs({boundary:2})));
});
test('duplicate or changed payload remains refused',async()=>{
  await assert.rejects(completeTurn(inputs({records:[user,reply,user,reply]})));
  await assert.rejects(completeTurn(inputs({records:[{...user,text:prompt+' '},reply]})));
});
test('reply in another human turn cannot satisfy this turn',async()=>{
  await assert.rejects(completeTurn(inputs({records:[user,{role:'user',text:'other payload'},reply]})));
});
test('assistant output without supplied marker cannot complete the turn',async()=>{
  await assert.rejects(completeTurn(inputs({records:[user,{...reply,text:'other marker'}]})));
});
test('failed reply boundary never reads native records or activity',async()=>{
  const config=inputs();
  config.waitReply=async()=>{throw new Error('reply deadline');};
  config.snapshot=async()=>assert.fail('snapshot before reply');
  config.readActivity=async()=>assert.fail('activity before reply');
  await assert.rejects(completeTurn(config),/reply deadline/);
});
test('separate Working failure allows later proved turns to complete',async()=>{
  const completed=[];
  for (let index=0;index<2;index++) completed.push(await completeTurn(inputs()));
  assert.equal(completed.length,2);
  assert.ok(completed.every(result=>result.working.status==='FAIL'));
});
test('different run roots change all consumed prompt identities',()=>{
  for (const purpose of ['0','1','attachment','stop']) {
    const first=runScopedIdentity('/evidence/run-a/b5-remote-codex','windows-web-to-mac','codex',purpose);
    const second=runScopedIdentity('/evidence/run-b/b5-remote-codex','windows-web-to-mac','codex',purpose);
    assert.notEqual(first,second);
    assert.equal(first,runScopedIdentity('/evidence/run-a/b5-remote-codex','windows-web-to-mac','codex',purpose));
  }
});
test('identity distinguishes actions and refuses unsafe filename parts',()=>{
  const out='/evidence/run/b5-remote-codex';
  const values=['0','attachment','stop'].map(purpose=>runScopedIdentity(out,'direction','codex',purpose));
  assert.equal(new Set(values).size,3);
  assert.throws(()=>runScopedIdentity(out,'direction','codex','../escape'));
});
