import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {runSteps,ownTurnVerdict} from './browser-steps.mjs';
const root=fs.mkdtempSync(path.join(os.tmpdir(),'browser-steps-'));
test.after(()=>fs.rmSync(root,{recursive:true}));
const base={origin:'http://127.0.0.1:18473',steps:[{id:'submit',verb:'click',args:['button','Submit fixture'],once:true}]};
test('uncertain click is called once, retained, and same run refuses before replay',async()=>{
 let calls=0;const out=path.join(root,'uncertain');const call=async()=>{calls++;throw Error('ack lost');};
 await assert.rejects(runSteps({...base,out,call}),/ack lost/);assert.equal(calls,1);
 assert.equal(JSON.parse(fs.readFileSync(path.join(out,'1-submit-failure.json'))).state,'action_unconfirmed');
 await assert.rejects(runSteps({...base,out,call}),/EEXIST/);assert.equal(calls,1);
});
test('failed state assertion stops before subsequent mutation',async()=>{
 let calls=0;await assert.rejects(runSteps({...base,out:path.join(root,'state'),steps:[{id:'ready',verb:'snapshot',args:[],expect:'right account'},...base.steps],call:async()=>{calls++;return {rawExit:0,stdout:'wrong account',stderr:''};}}),/observable/);assert.equal(calls,1);
});
test('foreign origin, production and undeclared mutations refuse before any call',async()=>{
 let calls=0;const call=async()=>{calls++;};
 for(const options of [{origin:'http://127.0.0.1:7685'},{steps:[{id:'away',verb:'goto',args:['https://example.com']}]},{steps:[{id:'send',verb:'click',args:['button','Send']}]}])await assert.rejects(runSteps({...base,...options,out:path.join(root,'bad'),call}));assert.equal(calls,0);
});
test('semantic steps preserve argv and complete only selected scope',async()=>{
 const seen=[];const out=path.join(root,'good');await runSteps({...base,out,call:async a=>{seen.push(a);return {rawExit:0,stdout:'observed',stderr:''};}});assert.deepEqual(seen,[['click','button','Submit fixture']]);assert.match(JSON.parse(fs.readFileSync(path.join(out,'completed.json'))).scope,/not whole-case/);
});
const v={completedReplyIds:['a'],status:'idle',session:'own',actualSession:'own',nonce:'bound',reportedNonce:'bound',beforeIds:['old'],payload:'opaque',messages:[{id:'old',type:'assistant',content:'stale'},{id:'u',type:'user',content:'opaque'},{id:'a',type:'assistant',content:'Any natural answer'}]};
test('own append proves structural reply without model wording expectation',()=>{assert.deepEqual(ownTurnVerdict(v),{state:'roundtrip_observed',userId:'u',replyIds:['a']});});
test('foreign binding and duplicated input do not pass',()=>{assert.equal(ownTurnVerdict({...v,actualSession:'foreign'}).state,'refused');assert.equal(ownTurnVerdict({...v,messages:[...v.messages,{id:'u2',type:'user',content:'opaque'}]}).state,'unconfirmed');});
test('old reply or later user reply cannot satisfy this turn',()=>{assert.equal(ownTurnVerdict({...v,messages:[v.messages[0],v.messages[1],{id:'u2',type:'user',content:'other'},v.messages[2]]}).state,'delivered');});

test('assistant progress while still working is not completed roundtrip',()=>{assert.equal(ownTurnVerdict({...v,status:'working',completedReplyIds:[]}).state,'delivered');});

test('idle alone, old completion and unrelated completion cannot prove final reply', () => {
  for (const completedReplyIds of [[], ['old'], ['foreign']]) {
    assert.equal(ownTurnVerdict({...v, completedReplyIds}).state, 'delivered');
  }
});

test('missing, reordered, truncated and duplicate-ID append windows refuse', () => {
  for (const options of [
    {beforeIds: undefined}, {beforeIds: ['missing']}, {beforeIds: ['old', 'u', 'a', 'extra']},
    {messages: [v.messages[1], v.messages[0], v.messages[2]]},
    {messages: [...v.messages, v.messages[2]]}, {messages: [{type: 'user', content: 'opaque'}]},
    {payload: ''}, {payload: 123},
  ]) assert.equal(ownTurnVerdict({...v, ...options}).state, 'refused');
});

test('nonce mismatch and stale matching payload cannot confirm delivery', () => {
  assert.equal(ownTurnVerdict({...v, reportedNonce: 'other'}).state, 'refused');
  assert.equal(ownTurnVerdict({...v, beforeIds: ['old', 'u', 'a']}).state, 'unconfirmed');
});

test('later user completion and blank assistant cannot complete selected turn', () => {
  assert.equal(ownTurnVerdict({...v, messages: [v.messages[0], v.messages[1],
    {id: 'next', type: 'user', content: 'other'}, v.messages[2]]}).state, 'delivered');
  assert.equal(ownTurnVerdict({...v, messages: [v.messages[0], v.messages[1],
    {...v.messages[2], content: ' '}]}).state, 'delivered');
});

test('nonzero and oversized mutation results stop with uncertainty and no subsequent action', async () => {
  for (const [id, result] of [
    ['nonzero', {rawExit: 1, stdout: '', stderr: 'refused'}],
    ['overflow', {rawExit: 0, stdout: 'x'.repeat(256 * 1024 + 1), stderr: ''}],
  ]) {
    let calls = 0;
    const out = path.join(root, id);
    await assert.rejects(runSteps({...base, out,
      steps: [...base.steps, {id: 'later', verb: 'snapshot', args: []}],
      call: async () => { calls++; return result; }}));
    assert.equal(calls, 1);
    assert.equal(JSON.parse(fs.readFileSync(path.join(out, '1-submit-failure.json'))).state, 'action_unconfirmed');
    assert.equal(fs.existsSync(path.join(out, 'completed.json')), false);
  }
});

test('unsafe locators, cross-owner flags, credentials and invalid bounds refuse before dispatch', async () => {
  let calls = 0;
  const steps = [
    {id: 'css', verb: 'click', args: ['#submit', 'Send'], once: true},
    {id: 'owner', verb: 'snapshot', args: ['--of', 'other']},
    {id: 'sleep', verb: 'wait', args: ['1000']},
    {id: 'ref', verb: 'click', args: ['capture:1'], once: true},
    {id: 'flag', verb: 'type', args: ['textbox', 'Name', '--stdin'], once: true},
    {id: 'creds', verb: 'goto', args: ['http://user:pass@127.0.0.1:18473/']},
  ];
  for (const step of steps) await assert.rejects(runSteps({...base, steps: [step],
    out: path.join(root, 'refused'), call: async () => { calls++; }}));
  for (const options of [{steps: []}, {steps: Array(31).fill(base.steps[0])},
    {steps: [...base.steps, ...base.steps]}, {origin: 'http://127.0.0.1:7686'}]) {
    await assert.rejects(runSteps({...base, ...options, out: path.join(root, 'refused'),
      call: async () => { calls++; }}));
  }
  assert.equal(calls, 0);
});

test('ordered fixture navigation, role wait and snapshots retain separate evidence', async () => {
  const steps = [
    {id: 'open', verb: 'goto', args: [base.origin + '/']},
    {id: 'snapshot', verb: 'snapshot', args: ['80'], expect: 'fixture'},
    {id: 'wait', verb: 'wait', args: ['--role', 'heading', '--name', 'Fixture submitted', '--state', 'visible', '--timeout', '15s']},
  ];
  const out = path.join(root, 'sequence');
  const seen = [];
  await runSteps({...base, steps, out, call: async args => {
    seen.push(args); return {rawExit: 0, stdout: 'fixture', stderr: ''};
  }});
  assert.deepEqual(seen, steps.map(step => [step.verb, ...step.args]));
  for (const [i, step] of steps.entries()) {
    assert.equal(fs.existsSync(path.join(out, `${i + 1}-${step.id}-attempt.json`)), true);
    assert.equal(fs.existsSync(path.join(out, `${i + 1}-${step.id}-result.json`)), true);
  }
});
