import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {parse} from 'yaml';
import {condition, evaluateCondition} from './workflow-condition.mjs';
import {verifyQualification, QUALIFICATION_JOBS} from '../ci-qualification.mjs';
import {checkQualifiedRun, isReleasePushRun, REQUIRED_JOBS} from '../release-promote.mjs';
import {ciFailures} from '../ci-result.mjs';

const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const ci = workflow('tests'), release = workflow('release');
const sha = 'a'.repeat(40);
const pr = {number:668, state:'open', merged:false, head:{ref:'release/1.15.3', sha, repo:{id:1,full_name:'domios-dev/domios'}},
  base:{ref:'master',repo:{id:1,full_name:'domios-dev/domios'}}};
const github = {workflow:'CI',ref:'refs/heads/release/1.15.3',ref_name:'release/1.15.3',repository:'domios-dev/domios',repository_owner:'domios-dev',actor:'rollacode',triggering_actor:'rollacode',
  event_name:'push',head_ref:pr.head.ref,base_ref:'master',event:{pull_request:pr}};
const run = {id:42,run_attempt:1,path:'.github/workflows/tests.yml',event:'push',head_sha:sha,head_branch:pr.head.ref,
  status:'completed',conclusion:'success',actor:{login:'rollacode'},triggering_actor:{login:'rollacode'},
  repository:{id:1,full_name:github.repository,owner:{login:'domios-dev'}},head_repository:pr.head.repo,pull_requests:[pr]};
const AsyncFunction = Object.getPrototypeOf(async function(){}).constructor;

test('every dependency and reusable input resolves in the PR release graph',()=>{
  for(const name of ['tests','release','tests-fast','tests-rust','rust-warnings-summary']) {
    const graph=workflow(name);
    for(const [id,job] of Object.entries(graph.jobs)) {
      for(const dependency of [job.needs??[]].flat()) assert.ok(graph.jobs[dependency],`${name}/${id}: ${dependency}`);
      if(job.uses?.startsWith('./.github/workflows/')) {
        const child=workflow(job.uses.split('/').at(-1).replace('.yml',''));
        for(const input of Object.keys(job.with??{})) assert.ok(child.on.workflow_call.inputs[input],`${name}/${id}: ${input}`);
      }
    }
  }
  const compiler=workflow('release-arm64-compile');
  assert.doesNotMatch(JSON.stringify(compiler),/secrets\.(APPLE_|TAURI_|GH_RELEASE|PACKAGE_REPOS)/);
  const report=workflow('rust-warnings-summary').jobs.report;
  const context={github,inputs:{report_name:'rust-warnings-release-report'},vars:{CI_LINUX_LIGHT_RUNNER:'ubuntu-24.04'}};
  assert.equal(evaluateCondition(report.environment,context),'release');
  assert.ok(evaluateCondition(report['runs-on'],context).includes('self-hosted'));
});

test('release push owns full CI and exact-head packaging while every PR stays light', () => {
  assert.ok(ci.on.push.branches.includes('release/**'));
  assert.ok(ci.on.pull_request.types.includes('edited'));
  assert.equal(release.on.workflow_run, undefined);
  assert.equal(release.on.pull_request, undefined);
  assert.deepEqual(release.on.push, {branches:['master']});
  const needs = {admission:{outputs:{run:'true',release_head:'true'}},policy:{result:'success'},
    fast:{result:'success',outputs:{resolved_scope:'{}',heavy:'true'}},rust:{result:'success'},'rust-warnings':{result:'success'},qualification:{result:'success'}};
  for (const id of ['fast','rust','rust-warnings','qualification','release-dry-run']) assert.ok(condition(ci.jobs[id].if,{github,needs,inputs:{scope:'',source_ref:''}}),id);
  assert.equal(ci.jobs['release-dry-run'].with.dry_run,true);
  assert.equal(evaluateCondition(ci.jobs['release-dry-run'].with.source_sha,{github:{...github,sha},needs:{}}),sha);
  for (const event_name of ['pull_request','pull_request_target']) {
    const request={...github,event_name,ref:'refs/pull/668/merge'};
    for(const name of ['tests-fast','tests-rust','rust-warnings-summary'])for(const [id,job]of Object.entries(workflow(name).jobs)){
      assert.equal(condition(job.if,{github:request,inputs:{resolved_scope:'{}'},needs:{changed:{outputs:{}}}}),false,name+'/'+id);
    }
  }
  const graph=Object.fromEntries(['policy','fast','rust','rust-warnings','qualification','release-dry-run'].map(id=>[id,{result:'success'}]));
  assert.deepEqual(ciFailures('push',graph,'',{releaseHead:true}),[]);
  for (const id of Object.keys(graph)) assert.ok(ciFailures('push',{...graph,[id]:{result:'skipped'}},'',{releaseHead:true}).includes(id),id);
  assert.equal(ci.concurrency['cancel-in-progress'],true);
  assert.equal(evaluateCondition(ci.concurrency.group,{github:{...github,run_id:1},inputs:{source_ref:''}}),evaluateCondition(ci.concurrency.group,{github:{...github,run_id:2},inputs:{source_ref:''}}));
  assert.ok(ci.jobs.result.needs.includes('release-dry-run'));
  assert.equal(evaluateCondition(ci.jobs.result.name,{github:{...github,event_name:'pull_request'}}),'Release PR checks');
  assert.equal(evaluateCondition(ci.jobs.result.name,{github}),'CI result');
  assert.equal(condition(ci.jobs.pr.if,{github:{...github,event_name:'pull_request'}}),false);
  assert.deepEqual(ciFailures('pull_request',{policy:{result:'success'},pr:{result:'skipped'}},'',{releasePrLight:true}),[]);

});

test('develop duplicate admission uses exact release push SHA, with a clear skip reason',async()=>{
  const script=ci.jobs.admission.steps[0].with.script;
  const execute=new AsyncFunction('github','context','core',script);
  for (const [rows,expected] of [[[run],'false'],[[{...run,head_sha:'b'.repeat(40)}],'true'],[[],'true']]) {
    const outputs={}, notices=[];
    await execute({paginate:async()=>rows,rest:{actions:{listWorkflowRuns(){}}}},
      {eventName:'push',ref:'refs/heads/develop',sha,repo:{owner:'domios-dev',repo:'domios'},payload:{}},
      {setOutput:(key,value)=>outputs[key]=value,notice:value=>notices.push(value)});
    assert.equal(outputs.run,expected);
    if(expected==='false')assert.match(notices[0],/release push run owns this exact SHA/);
  }
  const outputs={};
  await execute({}, {eventName:'push',ref:'refs/heads/master',repo:{},payload:{}},
    {setOutput:(key,value)=>outputs[key]=value,notice(){}});
  assert.equal(outputs.run,'false','master merge commits never acquire compilers');
  assert.deepEqual(ciFailures('push',{policy:{result:'success'},fast:{result:'skipped'}},'',{skipHeavy:true}),[]);

});

test('merge admission selects green retained PR head and refuses squash, unmerged or foreign heads',async()=>{
  const script=release.jobs['qualified-source'].steps[0].with.script;
  const execute=new AsyncFunction('github','context','core','process',script);
  const admitted={};
  await execute({}, {eventName:'push',ref:github.ref,sha}, {setOutput:(key,value)=>admitted[key]=value},
    {env:{DRY_RUN:'true',SOURCE_SHA:sha}});
  assert.deepEqual(admitted,{source_sha:sha});
  await assert.rejects(execute({}, {eventName:'push',ref:github.ref,sha}, {}, {env:{DRY_RUN:'true',SOURCE_SHA:'b'.repeat(40)}}),/Foreign release push head/);
  const merged={...pr,state:'closed',merged:true,merged_at:'2026-10-25',merge_commit_sha:'c'.repeat(40)};
  const invoke=async(candidate,parents,runs=[run])=>{
    const outputs={};
    await execute({rest:{repos:{getCommit:async()=>({data:{parents}}),listPullRequestsAssociatedWithCommit(){}},actions:{listWorkflowRuns(){}}},paginate:async(method,args)=>args.workflow_id?runs:[candidate]},
      {eventName:'push',ref:'refs/heads/master',sha:candidate.merge_commit_sha,repo:{owner:'domios-dev',repo:'domios'}},{setOutput:(key,value)=>outputs[key]=value,notice(){}}, {env:{DRY_RUN:''}});
    return outputs;
  };
  const parents=[{sha:'b'.repeat(40)},{sha}];
  assert.deepEqual(await invoke(merged,parents),{source_sha:sha,promote_run_id:'42',branch:pr.head.ref});
  await assert.rejects(invoke(merged,[{sha}]),/real merge/);
  await assert.rejects(invoke(merged,[parents[0],{sha:'d'.repeat(40)}]),/real merge/);
  assert.deepEqual(await invoke(pr,parents),{});
  for(const bad of [{...run,conclusion:'failure'},{...run,head_sha:'d'.repeat(40)},{...run,head_repository:{full_name:'fork/domios'}}])
    await assert.rejects(invoke(merged,parents,[bad]),/No green/);
  for(const id of ['frontend-dist','macos-arm64','macos-x64','linux-arm64','linux-x86_64','windows-x64','windows-arm64'])
    assert.equal(condition(release.jobs[id].if,{github,needs:{prepare:{result:'success',outputs:{promote_run_id:'42',kind:'app'}},'frontend-dist':{result:'success'}}}),false,id);
  assert.equal(release.jobs.promote,undefined);
});

test('Push qualification and retained packaging accept only same-repository exact-head evidence',()=>{
  assert.equal(isReleasePushRun(run,sha),true);
  for(const bad of [{...run,head_sha:'b'.repeat(40)},{...run,actor:{login:'fork'}},
    {...run,event:'pull_request'},{...run,head_repository:{full_name:'fork/domios'}}]) {
    assert.equal(isReleasePushRun(bad,sha),false);
    assert.throws(()=>checkQualifiedRun({run:bad,jobs:[],sourceSha:sha,branch:pr.head.ref}),/promotion refused/);
  }
  const jobs=REQUIRED_JOBS.map(name=>({name:'Release packaging / '+name+'build',conclusion:'success'}));
  checkQualifiedRun({run,jobs,sourceSha:sha,branch:pr.head.ref});
  assert.throws(()=>checkQualifiedRun({run,jobs:jobs.slice(1),sourceSha:sha,branch:pr.head.ref}),/no prepare/);
  const receipt={version:1,sha,run_id:42,run_attempt:1,results:{fast:'success',rust:'success',warnings:'success'},
    rust:[['full','Linux'],['platform','macOS'],['platform','Windows'],['embeddings','macOS']].map(([mode,os])=>({mode,os,sha,exit:0,executed:1,seconds:1}))};
  assert.equal(verifyQualification(receipt,run,QUALIFICATION_JOBS.map(name=>({name,conclusion:'success'})),sha).sha,sha);
  assert.throws(()=>verifyQualification(receipt,run,
    QUALIFICATION_JOBS.filter(name=>name!=='CI result').map(name=>({name,conclusion:'success'})),sha),/CI result/);
});
