import test from 'node:test';
import assert from 'node:assert/strict';
import {VERSION,measurementCapabilities,requireMeasurement,rendererMetrics,admitDevSurface,validateDevSurfaceTarget,segmentSeconds,validateIdentity,validateResume,cleanupPlan,validateFileOwnership,emitterDecision,sampleDue,acknowledge,muxBindings} from './model.mjs';
const run = '11111111-1111-1111-1111-111111111111';
const identity = () => ({port:7686,dev:true,page:{origin:'http://127.0.0.1:7686',path:'/'},capabilities:measurementCapabilities('profile',1),build:'a'.repeat(40),entryHash:'b'.repeat(64),target:'page',boot:'boot',renderer:{pid:1,created:'r'},daemon:{pid:2,created:'d'}});
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
test('replay ownership includes both isolated transcript fixtures and still refuses foreign cleanup',() => {
  const cp = checkpoint(); cp.mode = 'replay'; cp.fixtures.push(fixture('other','other-session'));
  const live = {...bindings(),other:{workspace_id:'owned',agent_session_id:'other-session',pty_id:'pty-other'}};
  assert.deepEqual(cleanupPlan(cp,identity(),live),['chat','other','terminal']);
  assert.throws(() => validateResume({...cp,fixtures:cp.fixtures.slice(0,1)},identity(),live));
  assert.throws(() => cleanupPlan(cp,identity(),{...live,foreign:{workspace_id:'owned'}}),/Foreign/);
  assert.throws(() => validateResume(cp,identity(),{...live,other:{...live.other,session_binding_nonce:'rebound'}}),/nonce/);
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


const split = () => ({health:{status:'ok',build_sha:'a'.repeat(40),instance:{name:'',api_port:7686}},
  provenance:{schema:3,buildSha:'a'.repeat(40),instance:{name:'',apiPort:'7686',vitePort:'1420',cdpPort:'9223',identifier:'com.codeterm.dev'}},
  surface:'chrome',cdp:58454,target:'owned-page'});
const browserTarget = origin => ({id:'owned-page',type:'page',url:origin+'/',webSocketDebuggerUrl:'ws://127.0.0.1:58454/devtools/page/owned-page'});

test('split Browser admission pins exact server provenance and API-served target independently',() => {
  const input = split(), admitted = admitDevSurface(input);
  assert.equal(admitted.origin,'http://127.0.0.1:7686');
  assert.equal(validateDevSurfaceTarget(admitted,browserTarget(admitted.origin)).id,'owned-page');
  for (const provenance of [null,{...input.provenance,schema:2},{...input.provenance,buildSha:'b'.repeat(40)},
    {...input.provenance,instance:{...input.provenance.instance,name:'foreign'}},
    {...input.provenance,instance:{...input.provenance.instance,apiPort:'7685'}},
    {...input.provenance,instance:{...input.provenance.instance,identifier:'com.codeterm'}}])
    assert.throws(() => admitDevSurface({...input,provenance}));
  for (const health of [{...input.health,status:'starting'},{...input.health,build_sha:'a123'},
    {...input.health,build_sha:'b'.repeat(40)},{...input.health,instance:{name:'foreign',api_port:7686}},
    {...input.health,instance:{name:'',api_port:7685}}]) assert.throws(() => admitDevSurface({...input,health}));
  for (const cdp of [undefined,0,7685,7686,65536]) assert.throws(() => admitDevSurface({...input,cdp}));
  assert.throws(() => admitDevSurface({...input,target:null}));
  const target = browserTarget(admitted.origin);
  for (const patch of [{id:'foreign'},{type:'worker'},{url:'http://127.0.0.1:1420/'},{url:'http://127.0.0.1:7685/'},
    {webSocketDebuggerUrl:'ws://127.0.0.1:9999/devtools/page/owned-page'}])
    assert.throws(() => validateDevSurfaceTarget(admitted,{...target,...patch}));
});

test('split native surface refuses missing owning locator while legacy target checks remain strict',() => {
  const input = split();
  assert.deepEqual(admitDevSurface({...input,surface:'native'}),{available:false,reason:'Native CDP locator unavailable for independent server'});
  const legacy = {...input,health:{...input.health,instance:{...input.health.instance,
    identifier:'com.codeterm.dev',vite_port:'1420',cdp_port:'9223'}}};
  assert.equal(admitDevSurface(legacy).origin,'http://127.0.0.1:1420');
  const native = admitDevSurface({...legacy,surface:'native'});
  assert.equal(native.cdpPort,9223);
  assert.throws(() => admitDevSurface({...legacy,cdp:9223}));
  for (const patch of [{identifier:'com.codeterm'},{vite_port:'9999'},{cdp_port:''}])
    assert.throws(() => admitDevSurface({...legacy,health:{...legacy.health,instance:{...legacy.health.instance,...patch}}}));
});


test('functional admission preserves page identity while OS renderer metrics are explicitly unavailable',() => {
  const actual = {...identity(),capabilities:measurementCapabilities('functional',5),renderer:null};
  assert.equal(validateIdentity(actual),actual);
  assert.deepEqual(rendererMetrics(actual,null),{renderer:null,rendererPrivateBytes:null,
    rendererAttribution:{available:false,reason:'functional_page_only'}});
  assert.throws(() => rendererMetrics(actual,{pid:99,created:'arbitrary'}));
  for (const kind of ['memory','cpu','trace']) assert.throws(() => requireMeasurement(actual,kind),/unavailable/);
  for (const count of [0,2,5,undefined]) assert.throws(() => measurementCapabilities('profile',count),/Exactly one/);
  assert.doesNotThrow(() => requireMeasurement(identity(),'trace'));
});

test('functional resume and cleanup fence capability and page drift without a renderer guess',() => {
  const actual = {...identity(),capabilities:measurementCapabilities('functional'),renderer:null};
  const cp = {...checkpoint(),identity:actual};
  assert.equal(validateResume(cp,actual,bindings()),cp);
  assert.deepEqual(cleanupPlan(cp,actual,bindings()),['chat','terminal']);
  assert.throws(() => validateResume(cp,identity(),bindings()),/drift/);
  assert.throws(() => validateResume(cp,{...actual,target:'foreign'},bindings()),/drift/);
  assert.throws(() => validateResume(cp,{...actual,page:{...actual.page,path:'/other'}},bindings()),/drift/);
  assert.throws(() => validateIdentity({...actual,page:null}));
  assert.throws(() => validateIdentity({...actual,renderer:{pid:99,created:'guessed'}}));
  assert.throws(() => validateIdentity({...identity(),renderer:null}));
});
