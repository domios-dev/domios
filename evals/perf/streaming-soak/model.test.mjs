import test from 'node:test';
import assert from 'node:assert/strict';
import {VERSION,segmentSeconds,validateIdentity,validateResume,cleanupPlan,validateFileOwnership,emitterDecision,sampleDue,acknowledge,muxBindings} from './model.mjs';
const run = '11111111-1111-1111-1111-111111111111';
const identity = () => ({port:7686,dev:true,build:'a'.repeat(40),entryHash:'b'.repeat(64),target:'page',boot:'boot',renderer:{pid:1,created:'r'},daemon:{pid:2,created:'d'}});
const fixture = (tab,session) => ({run,tab,session,pty:'pty-'+tab});
const checkpoint = () => ({version:VERSION,stage:'ready',run,identity:identity(),batch:0,pending:null,workspace:{id:'owned'},
  fixtures:[fixture('chat','session')],terminal:fixture('terminal',null)});
const bindings = () => ({chat:{workspace_id:'owned',agent_session_id:'session',pty_id:'pty-chat'},terminal:{workspace_id:'owned',agent_session_id:null,pty_id:'pty-terminal'}});
test('mux ownership derives from workspace tab lists, never an invented tab field',() => {
  const state = {workspaces:[{id:'owned',tabs:['chat']}],tabs:{chat:{pty_id:'p',workspace_id:'foreign'},loose:{pty_id:'q'}}};
  assert.equal(muxBindings(state).chat.workspace_id,'owned'); assert.equal(muxBindings(state).loose.workspace_id,null);
  assert.throws(() => muxBindings({...state,workspaces:[...state.workspaces,{id:'foreign',tabs:['chat']}]}));
});

test('segments are finite and capped at eight minutes',() => {
  assert.equal(segmentSeconds(480),480);
  for (const v of [0,-1,481,Infinity,NaN,1.5,'1']) assert.throws(() => segmentSeconds(v));
});
test('DEV identity refuses PROD, missing fields and imprecise SHA',() => {
  assert.equal(validateIdentity(identity()).port,7686);
  for (const patch of [{port:7685},{dev:false},{build:'a123'},{entryHash:null},{renderer:{pid:1}},{daemon:{pid:0,created:'d'}},{target:null},{boot:null}])
    assert.throws(() => validateIdentity({...identity(),...patch}));
});
test('resume accepts exact ownership and refuses every identity drift',() => {
  assert.equal(validateResume(checkpoint(),identity(),bindings()).batch,0);
  for (const key of ['build','entryHash','target','boot']) {
    const actual = identity(); actual[key] = key === 'build'?'c'.repeat(40):key === 'entryHash'?'d'.repeat(64):'changed';
    assert.throws(() => validateResume(checkpoint(),actual,bindings()),/drift/);
  }
  for (const key of ['renderer','daemon']) for (const field of ['pid','created']) {
    const actual = identity(); actual[key][field] = field === 'pid'?999:'replacement';
    assert.throws(() => validateResume(checkpoint(),actual,bindings()),/drift/);
  }
});
test('resume refuses unsupported, unfinished, foreign or rebound checkpoints',() => {
  for (const patch of [{version:2},{stage:'setup'},{batch:-1},{pending:{batch:1}},{run:'foreign'},{fixtures:[]}])
    assert.throws(() => validateResume({...checkpoint(),...patch},identity(),bindings()));
  for (const field of ['workspace_id','agent_session_id','pty_id']) {
    const live = bindings(); live.chat[field] = 'foreign';
    assert.throws(() => validateResume(checkpoint(),identity(),live));
  }
  const cp = checkpoint(); delete cp.fixtures[0].pty; delete bindings().chat.pty_id;
  assert.throws(() => validateResume(cp,identity(),bindings()));
  const duplicated = checkpoint(); duplicated.terminal.tab = 'chat';
  assert.throws(() => validateResume(duplicated,identity(),bindings()));
});
test('cleanup lists only checkpoint-owned tabs and refuses foreign additions',() => {
  assert.deepEqual(cleanupPlan(checkpoint(),identity(),bindings()),['chat','terminal']);
  assert.throws(() => cleanupPlan(checkpoint(),identity(),{...bindings(),foreign:{workspace_id:'owned'}}),/Foreign/);
  assert.deepEqual(cleanupPlan(checkpoint(),identity(),{...bindings(),foreign:{workspace_id:'other'}}),['chat','terminal']);
});
test('filesystem cleanup gate checks owner, target, inventory and contents',() => {
  const cp = {...checkpoint(),history:'owned-history'}; cp.fixtures[0].path = 'file'; cp.fixtures[0].hash = 'hash';
  const actual = {owner:run,history:'owned-history',junctionMatches:true,inventoryMatches:true,hashes:{file:'hash'}};
  assert.doesNotThrow(() => validateFileOwnership(cp,actual));
  for (const patch of [{owner:'other'},{history:'other'},{junctionMatches:false},{inventoryMatches:false},{hashes:{file:'changed'}}])
    assert.throws(() => validateFileOwnership(cp,{...actual,...patch}));
});
test('protocol accepts exact next batch, bounded output, status and stop',() => {
  const state = {run,token:'secret',batch:7,stopped:false};
  const message = {version:1,run,token:'secret',op:'batch',batch:8,lines:128};
  assert.deepEqual(emitterDecision(state,message),{op:'batch',batch:8,lines:128});
  for (const patch of [{version:2},{run:'foreign'},{token:'foreign'},{batch:7},{batch:9},{lines:0},{lines:129},{lines:1.5},{op:'provider'}])
    assert.throws(() => emitterDecision(state,{...message,...patch}));
  assert.deepEqual(emitterDecision(state,{...message,op:'stop'}),{op:'stop',batch:7});
  assert.throws(() => emitterDecision({...state,stopped:true},message));
  assert.deepEqual(emitterDecision({...state,stopped:true},{...message,op:'status'}),{op:'status',batch:7});
});
test('elapsed cadence schedules samples but never proves readiness or ack',() => {
  assert.equal(sampleDue(1,null),true); assert.equal(sampleDue(59_999,0),false); assert.equal(sampleDue(60_000,0),true);
  assert.doesNotThrow(() => acknowledge(8,{ok:true,batch:8}));
  for (const reply of [null,{ok:true,batch:7},{ok:false,batch:8},{ok:true,batch:'8'}]) assert.throws(() => acknowledge(8,reply));
});
