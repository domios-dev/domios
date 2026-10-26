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

