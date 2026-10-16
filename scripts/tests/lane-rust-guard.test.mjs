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
 const proof=readGateProof({CODETERM_PAD_DIR:pad,CODETERM_PLAN_ID:'pl_test',CODETERM_TASK_ID:'task_test'},p=>{seen.push(p);return p.endsWith('plan.yaml')?'id: pl_test\nowner: {tab_id: owner}':'id: task_test\nexecution_attempts: []';},()=>['task.yaml','task.jsonl']);
 assert.equal(proof.plan.id,'pl_test');assert.equal(proof.tasks[0].id,'task_test');assert.equal(seen.length,2);
 assert.ok(seen.every(p=>p.replaceAll('\\','/').includes('/instance/plans/active/pl_test/')));
});
test('absent offline identity refuses',()=> {
 assert.equal(readGateProof({}),null);
 assert.equal(readGateProof({CODETERM_PLAN_ID:'../../foreign',CODETERM_PAD_DIR:'/runtime/agent-pads/tab'}),null);
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
