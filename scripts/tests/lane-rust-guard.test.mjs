import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { authorizedRustGate, gateCommand, readGateProof } from '../rust-gate-authorization.mjs';
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const args = ['nextest', 'plugin', 'test(/^parity::/)'];
function fixture() {
 return {
  env: {CODETERM_ROLE:'Worker', CODETERM_PLAN_ID:'pl_test', CODETERM_TASK_ID:'task_test', CODETERM_ATTEMPT_ID:'attempt_test', CODETERM_TAB_ID:'worker', CODETERM_SESSION_BINDING_NONCE:'nonce'},
  current:'/lane', canonical:'/canonical', args,
  plan: {id:'pl_test', owner:{tab_id:'owner'}},
  tasks: [{id:'task_test', assigned_tab_id:'worker', interruption:{owner_epoch:2}, execution_attempts:[{id:'attempt_test', executor_tab_id:'worker', session_binding_nonce:'nonce', interruption:{owner_epoch:2}, authorized_gates:[gateCommand(args)]}]}],
 };
}
for (const canonical of [false,true]) {
 test(`exact grant passes; canonical=${canonical}`,()=> {
  const f=fixture(); if(canonical) f.current=f.canonical;
  assert.equal(authorizedRustGate(f),true);
 });
 for(const reason of ['missing','empty','different command','closed','retired','wrong tab','wrong nonce','wrong attempt','wrong task','wrong epoch','unassigned','wrong plan']) {
  test(`refuses ${reason}; canonical=${canonical}`,()=> {
   const f=fixture(); if(canonical) f.current=f.canonical;
   const a=f.tasks[0].execution_attempts[0];
   if(reason==='missing') delete a.authorized_gates;
   if(reason==='empty') a.authorized_gates=[];
   if(reason==='different command') f.args=[...args,'--peer','auto'];
   if(reason==='closed') a.closed_at=1;
   if(reason==='retired') a.retired_at=1;
   if(reason==='wrong tab') a.executor_tab_id='other';
   if(reason==='wrong nonce') a.session_binding_nonce='other';
   if(reason==='wrong attempt') f.env.CODETERM_ATTEMPT_ID='other';
   if(reason==='wrong task') f.env.CODETERM_TASK_ID='other';
   if(reason==='wrong epoch') a.interruption.owner_epoch=3;
   if(reason==='unassigned') f.tasks[0].assigned_tab_id='other';
   if(reason==='wrong plan') f.plan.id='other';
   f.env.CODETERM_ALLOW_LANE_RUST_TESTS='1';
   assert.equal(authorizedRustGate(f),false);
  });
 }
}
for (const role of ['Orchestrator','', 'Worker','Planner']) {
 for (const canonical of [true,false]) {
  test(`owner/shell role=${role}, canonical=${canonical}`,()=> {
   const f=fixture(); f.current=canonical?f.canonical:'/lane';
   f.env={CODETERM_ROLE:role,CODETERM_PLAN_ID:'pl_test',CODETERM_TAB_ID:role==='Orchestrator'?'owner':'shell',CODETERM_PARENT_TAB_ID:'owner'};
   assert.equal(authorizedRustGate(f),canonical && ['Orchestrator',''].includes(role));
  });
 }
}
test('offline proof uses host pad instance and persisted task attempts',()=> {
 const pad=process.platform==='win32'?'C:/instance/runtime/agent-pads/tab':'/instance/runtime/agent-pads/tab';
 const seen=[];
 const proof=readGateProof({CODETERM_PAD_DIR:pad,CODETERM_PLAN_ID:'pl_test',CODETERM_TASK_ID:'task_test'},p=>{seen.push(p);return p.endsWith('plan.yaml')?'id: pl_test\nowner: {tab_id: owner}':'id: task_test\nexecution_attempts: []';},()=>['task.yaml','task.jsonl'],undefined,text=>text.startsWith('id: pl_test')?{id:'pl_test',owner:{tab_id:'owner'}}:{id:'task_test',execution_attempts:[]});
 assert.equal(proof.plan.id,'pl_test');assert.equal(proof.tasks[0].id,'task_test');assert.equal(seen.length,2);
 assert.ok(seen.every(p=>p.replaceAll('\\','/').includes('/instance/plans/active/pl_test/')));
});
test('task-less verification Workers receive only daemon-recorded spawn gates in canonical',()=> {
 const args=['test-one','app','worker_role_contract'];
 const grant=gateCommand(args);
 for(const profile of [null,'qa','release_manager']) {
  const env={CODETERM_ROLE:'Worker',CODETERM_TAB_ID:'worker-tab',CODETERM_AUTHORIZED_GATES:gateCommand(['verify'])};
  const record={id:'worker-tab',role:'Worker',role_profile:profile,parent_tab_id:'parent-tab',task_binding:null,taskless_authorized_gates:[grant]};
  const proof=readGateProof(env,undefined,undefined,(binary,argv,options)=> {
   assert.equal(binary,'codeterm');
   assert.deepEqual(argv,['--json','tab','inspect','--tab','worker-tab']);
   assert.equal(options.env.CODETERM_AUTHORIZED_GATES,env.CODETERM_AUTHORIZED_GATES);
   return JSON.stringify({data:[record]});
  });
  assert.deepEqual(proof.taskless_worker,record);
  assert.equal(authorizedRustGate({env,current:'/canonical',canonical:'/canonical',args,...proof}),true);
 }
});
test('task-less gate grants refuse off-canonical, unrecorded, task-bound, and unrelated Workers',()=> {
 const args=['test-one','app','worker_role_contract'];
 const env={CODETERM_ROLE:'Worker',CODETERM_TAB_ID:'worker-tab',CODETERM_AUTHORIZED_GATES:gateCommand(args)};
 const accepted={id:'worker-tab',role:'Worker',role_profile:'qa',parent_tab_id:'parent-tab',taskless_authorized_gates:[gateCommand(args)]};
 const proofFor=record=>readGateProof(env,undefined,undefined,()=>JSON.stringify({data:[record]}));
 const proof=proofFor(accepted);
 assert.equal(authorizedRustGate({env,current:'/lane',canonical:'/canonical',args,...proof}),false);
 assert.equal(authorizedRustGate({env,current:'/canonical',canonical:'/canonical',args,...proofFor({...accepted,taskless_authorized_gates:[gateCommand(['verify'])]})}),false);
 assert.equal(authorizedRustGate({env:{...env,CODETERM_TASK_ID:'task-bound'},current:'/canonical',canonical:'/canonical',args,...proof}),false);
 for(const changes of [
  {role:'Manager'},
  {parent_tab_id:null},
  {task_binding:{task_id:'task-bound'}},
  {taskless_authorized_gates:[]},
 ]) {
  const invalid=proofFor({...accepted,...changes});
  assert.ok(invalid===null || invalid.taskless_worker==null);
  assert.equal(authorizedRustGate({env,current:'/canonical',canonical:'/canonical',args,...invalid}),false);
 }
});
test('absent offline identity refuses',()=> {
 assert.equal(readGateProof({}),null);
 assert.equal(readGateProof({CODETERM_PLAN_ID:'../../foreign',CODETERM_PAD_DIR:'/runtime/agent-pads/tab'}),null);
});

test('task-bound workspace grant reads current host attempt instead of stale YAML',()=> {
 const f=fixture(); f.args=['nextest','workspace'];
 f.tasks[0].execution_attempts[0].authorized_gates=[gateCommand(f.args)];
 const proof=readGateProof(f.env,()=>{throw new Error('stale YAML must not be read');},undefined,(binary,argv)=>{
  assert.equal(binary,'codeterm');
  assert.deepEqual(argv,['plan','task','show','pl_test','task_test','--json']);
  return JSON.stringify({data:{task:f.tasks[0]}});
 });
 assert.equal(authorizedRustGate({...f,...proof}),true);
 for(const args of [['nextest','plugin'],['nextest','workspace','all()'],['test-one','app','tests::'],['check-queue']]) {
  assert.equal(authorizedRustGate({...f,...proof,args}),false);
 }
 assert.throws(()=>readGateProof(f.env,undefined,undefined,()=>{throw new Error('host unavailable');}),/host unavailable/);
});
test('shell guard fails closed with exact message when offline proof is unavailable',()=> {
 const guard=readFileSync(new URL('../lane-rust-guard.sh',import.meta.url),'utf8');
 const result=spawnSync(bash,['-c',`${guard}\ngit() { printf '/canonical\\n'; }\nunset CODETERM_PAD_DIR\nCODETERM_ALLOW_LANE_RUST_TESTS=1\nrequire_canonical_rust_tests nextest plugin`],{encoding:'utf8'});
 assert.equal(result.status,2);assert.equal(result.stdout,'');
 assert.equal(result.stderr.trim(),'Rust tests and check-queue run in canonical through the Orchestrator; hand over the selector');
});
test('build passes exact argv before setup; lane-check remains outside this guard',()=> {
 const source=readFileSync(new URL('../../build.sh',import.meta.url),'utf8');
 const call=source.indexOf('require_canonical_rust_tests "$CMD" "${@:2}" || exit $?');
 assert.ok(call>0 && call<source.indexOf('case "$CMD" in'));
 const guard=readFileSync(new URL('../lane-rust-guard.sh',import.meta.url),'utf8');
 const result=spawnSync(bash,['-c',`${guard}\nrequire_canonical_rust_tests lane-check fixture`],{encoding:'utf8'});
 assert.equal(result.status,0);assert.equal(result.stdout+result.stderr,'');
});
test('grant tokens preserve filter boundaries and reject a broader command',()=> {
 assert.equal(gateCommand(['nextest','plugin','test(/^parity::/)']), "./build.sh nextest plugin 'test(/^parity::/)'" );
 for(const args of [['test-one','plugin','tests::one'],['check-queue','abc123','--lane']]) {
  const f=fixture(); f.args=args; f.tasks[0].execution_attempts[0].authorized_gates=[gateCommand(args)];
  assert.equal(authorizedRustGate(f),true);
  f.args=args.slice(0,-1);assert.equal(authorizedRustGate(f),false);
 }
});
