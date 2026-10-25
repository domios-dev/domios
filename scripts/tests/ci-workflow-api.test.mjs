import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import {parse} from 'yaml';
import {checkRunnerTools} from '../ci-runner-paths.mjs';
import {QUALIFICATION_JOBS} from '../ci-qualification.mjs';
import {cancelWorkflow,listFailureJobs,verifyIssueReferences,requireQualificationWithGithub} from '../ci-workflow-api.mjs';
import {cancelRun} from '../ci-first-failure.mjs';

const workflows=new URL('../../.github/workflows/',import.meta.url);
const read=name=>parse(fs.readFileSync(new URL(name+'.yml',workflows),'utf8'));
test('probe errors preserve the real command, error code, stderr and stdout without longer deadlines',()=>{
  assert.throws(()=>checkRunnerTools('cargo',{env:{},execute:(command,args,options)=>{
    assert.equal(command,'cargo');assert.deepEqual(args,['--version']);assert.equal(options.timeout,2000);
    return{status:1,stderr:'opaque rustup component refusal',stdout:'opaque diagnostic'};
  }}),error=>error.message.includes('cargo --version (exit 1)')&&error.message.includes('opaque rustup component refusal')&&error.message.includes('opaque diagnostic'));
  assert.throws(()=>checkRunnerTools('gh',{env:{},execute:(_command,args)=>{
    assert.deepEqual(args,['--version']);return{status:null,error:{code:'ETIMEDOUT'},stderr:'opaque timeout output'};
  }}),/ETIMEDOUT.*\nopaque timeout output/);
});

test('all pinned Rust setup actions provision the checkout components before its proxy is probed',()=>{
  for(const file of fs.readdirSync(workflows).filter(file=>file.endsWith('.yml'))){
    const graph=parse(fs.readFileSync(new URL(file,workflows),'utf8'));
    for(const job of Object.values(graph.jobs))for(const step of job.steps||[]){
      if(step.uses?.startsWith('dtolnay/rust-toolchain@')&&step.with?.toolchain==='1.99.1'){
        assert.deepEqual(new Set(step.with.components.split(',')),new Set(['clippy','rustfmt']),file);
      }
    }
  }
});

test('CI result depends only on needs and optional Octokit enrichment cannot fail it',()=>{
  const job=read('tests').jobs.result;
  assert.ok(!job.steps.some(step=>step.name==='Runner preflight'));
  assert.ok(!job.steps.some(step=>/\bgh\s/.test(step.run||'')));
  const enrichment=job.steps.find(step=>step.name==='Read named job failures and timeouts');
  assert.equal(enrichment['continue-on-error'],true);
  assert.ok(enrichment.uses.startsWith('actions/github-script@'));
  const source=fs.readFileSync(new URL('../ci-result.mjs',import.meta.url),'utf8');
  assert.doesNotMatch(source,/child_process|\bfetch\b|ci-job-failures/);
});

test('workflow CLI calls are limited to an explicit publication allowlist',()=>{
  const allowed=JSON.parse(fs.readFileSync(new URL('../workflow-gh-publication-allowlist.json',import.meta.url),'utf8'));
  const found=[];
  for(const file of fs.readdirSync(workflows).filter(file=>file.endsWith('.yml'))){
    const graph=parse(fs.readFileSync(new URL(file,workflows),'utf8'));
    for(const[id,job]of Object.entries(graph.jobs))for(const step of job.steps||[]){
      const run=(step.run||'').split('\n').filter(line=>!line.trim().startsWith('#')).join('\n');
      if(!/\bgh\s+/.test(run))continue;
      const key=file+':'+id+':'+step.name;found.push(key);assert.ok(allowed[key]?.length>20,key);
    }
  }
  assert.deepEqual(found.sort(),Object.keys(allowed).sort());
  for(const name of ['tests','tests-fast','tests-pr','tests-rust','rust-warnings-summary']){
    assert.ok(!JSON.stringify(read(name)).includes('ci-runner-paths.mjs gh'));
  }
});

test('Octokit cancellation and issue verification use the exact runtime repository identities',async()=>{
  const calls=[];
  const github={rest:{actions:{cancelWorkflowRun:async args=>calls.push(args)},issues:{get:async args=>calls.push(args)}}};
  await cancelWorkflow(github,{repo:{owner:'fixture',repo:'source'},runId:41});
  await verifyIssueReferences(github,{issues:[{reference:'fixture/public#7'}],repository_issues:[{reference:'fixture/source#9'}]});
  assert.deepEqual(calls,[{owner:'fixture',repo:'source',run_id:41},{owner:'fixture',repo:'public',issue_number:7},{owner:'fixture',repo:'source',issue_number:9}]);
  await cancelRun({GITHUB_REPOSITORY:'fixture/source',GITHUB_RUN_ID:'41',GITHUB_TOKEN:'opaque'},async(url,options)=>{
    assert.equal(url,'https://api.github.com/repos/fixture/source/actions/runs/41/cancel');
    assert.equal(options.method,'POST');return{ok:true};
  });
});

test('authenticated receipt reads preserve exact-SHA, run-attempt and every-oracle verification',async()=>{
  const sha='a'.repeat(40),context={repo:{owner:'fixture',repo:'source'}};
  const run={id:17,run_attempt:2,path:'.github/workflows/tests.yml',event:'push',head_branch:'develop',head_sha:sha,status:'completed',conclusion:'success'};
  const receipt={version:1,sha,run_id:'17',run_attempt:2,results:{fast:'success',rust:'success',warnings:'success'},rust:[['full','Linux'],['platform','macOS'],['platform','Windows'],['embeddings','macOS']].map(([mode,os])=>({mode,os,sha,exit:0,executed:1,seconds:1}))};
  const actions={
    listArtifactsForRepo:async()=>({data:{artifacts:[{id:20,name:'qualification-'+sha,workflow_run:{id:17}}]}}),
    getWorkflowRun:async()=>({data:run}),
    listJobsForWorkflowRunAttempt:async()=>({data:{jobs:QUALIFICATION_JOBS.map(name=>({name,conclusion:'success'})),total_count:QUALIFICATION_JOBS.length}}),
    downloadArtifact:async()=>({data:Buffer.from('opaque')}),
  };
  assert.equal((await requireQualificationWithGithub({rest:{actions}},context,sha,()=>receipt)).sha,sha);
  await assert.rejects(requireQualificationWithGithub({rest:{actions}},context,sha,()=>({...receipt,run_attempt:1})),/stale/);
});

test('optional job enrichment preserves timeout and cancellation names',async()=>{
 const calls=[];
 const api={rest:{actions:{listJobsForWorkflowRunAttempt:'opaque-method'}},paginate:async(method,args)=>{
  calls.push({method,args});return[{name:'fixture timeout',conclusion:'timed_out'},{name:'fixture cancelled',conclusion:'cancelled'},{name:'fixture success',conclusion:'success'}];
 }};
 const result=await listFailureJobs(api,{repo:{owner:'fixture',repo:'source'},runId:41});
 assert.deepEqual(result,['fixture timeout: timed_out','fixture cancelled: cancelled']);
 assert.equal(calls[0].args.run_id,41);
 assert.equal(calls[0].args.per_page,100);
});

test('Tripwire can cancel even when checkout failed before its source helper existed',async()=>{
 const hook=read('tests-fast').jobs.changed.steps.find(step=>step.name==='Notify and cancel on first failure');
 const execute=new (Object.getPrototypeOf(async function(){}).constructor)('require','process','github','context','core',hook.with.script);
 const calls=[];
 await execute(name=>{assert.equal(name,'node:url');return{pathToFileURL:value=>new URL('file:///absent-checkout/'+value)};},
  {env:{GITHUB_WORKSPACE:'unavailable-checkout'}},
  {rest:{actions:{cancelWorkflowRun:async args=>calls.push(args)}}},{repo:{owner:'fixture',repo:'source'},runId:41},{warning:()=>{}});
 assert.deepEqual(calls,[{owner:'fixture',repo:'source',run_id:41}]);
});
