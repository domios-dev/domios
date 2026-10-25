import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import test from 'node:test';
import {parse} from 'yaml';
import {condition,evaluateCondition} from './workflow-condition.mjs';
import {scopeForRun} from '../ci-scope.mjs';
import {ciFailures} from '../ci-result.mjs';

const read=name=>parse(fs.readFileSync(new URL('../../.github/workflows/'+name+'.yml',import.meta.url),'utf8'));
const ci=read('tests'),fast=read('tests-fast');
const sha='a'.repeat(40);
const diagnostic={frontend:'true',rust:'true',heavy:'false',plugins:'[]',source_sha:sha,resolved_scope:'opaque'};
const needs={
 policy:{result:'success'},fast:{result:'success',outputs:diagnostic},
 pr:{result:'skipped'},rust:{result:'skipped'},'rust-warnings':{result:'skipped'},
 qualification:{result:'skipped'},'runner-preflight':{result:'skipped'},
};
const context=ref=>({
 github:{event_name:'workflow_dispatch',repository:'domios-dev/domios',repository_owner:'domios-dev',actor:'rollacode',ref,sha},
 inputs:{scope:'fast',source_ref:sha},needs:{...needs,changed:{outputs:diagnostic}},
});

test('fast dispatch selects every frontend contract without broad Rust compilation for any diff',()=>{
 for(const files of [[],['AGENTS.md'],['src/App.tsx'],['cli/src/main.rs']]){
  assert.deepEqual(scopeForRun(files,[],true,false,false,true),{frontend:true,rust:true,heavy:false,plugins:[]});
 }
 const scope=fast.jobs.changed.steps.find(step=>step.id==='scope');
 assert.equal(evaluateCondition(scope.env.CI_FAST_DISPATCH,context('refs/heads/ci/try-cleanup')),true);
 assert.ok(ci.on.workflow_dispatch.inputs.scope.options.includes('fast'));
});

test('any trusted source branch can run fast contracts, while PR and untrusted actors retain their boundary',()=>{
 for(const ref of ['refs/heads/ci/try-cleanup','refs/heads/topic/fixture','refs/heads/release/1.15.3']){
  const ctx=context(ref);
  for(const id of ['policy','fast'])assert.equal(condition(ci.jobs[id].if,ctx),true,id);
  for(const id of ['changed','guards','ts-static','frontend-dist','frontend-windows','ts-types','ts-unit','ts-vitest','ts-plugins','cli-tests']){
   assert.equal(condition(fast.jobs[id].if,ctx),true,id);
  }
  assert.equal(condition(fast.jobs['clippy-linux'].if,ctx),false);
 }
 for(const github of [{event_name:'pull_request'},{actor:'untrusted'},{repository_owner:'fork'}]){
  const ctx=context('refs/heads/ci/try-cleanup');ctx.github={...ctx.github,...github};
  for(const id of ['changed','guards','cli-tests'])assert.equal(condition(fast.jobs[id].if,ctx),false,id);
 }
});

test('a fast dispatch cannot run Rust suites or write a qualification receipt even with green sibling outputs',()=>{
 for(const id of ['rust','rust-warnings','qualification','runner-preflight','pr']){
  assert.equal(condition(ci.jobs[id].if,context('refs/heads/ci/try-cleanup')),false,id);
 }
 const ctx=context('refs/heads/ci/try-cleanup');
 ctx.needs.fast.outputs={...diagnostic,heavy:'true'};
 for(const id of ['rust','rust-warnings','qualification'])assert.equal(condition(ci.jobs[id].if,ctx),false,id);
 assert.deepEqual(ciFailures('workflow_dispatch',needs,'fast'),[]);
 for(const id of ['rust','qualification']){
  assert.ok(ciFailures('workflow_dispatch',{...needs,[id]:{result:'success'}},'fast').includes(id));
 }
 assert.ok(ciFailures('workflow_dispatch',{...needs,fast:{result:'failure'}},'fast').includes('fast'));
});

test('the aggregate states that a successful fast dispatch is diagnostic and does not qualify the SHA',()=>{
 const directory=fs.mkdtempSync(path.join(os.tmpdir(),'ci-fast-summary-'));
 try{
  const summary=path.join(directory,'summary.md');
  const result=spawnSync(process.execPath,['scripts/ci-result.mjs'],{encoding:'utf8',env:{
   ...process.env,CI_EVENT:'workflow_dispatch',CI_SCOPE:'fast',CI_NEEDS:JSON.stringify(needs),GITHUB_STEP_SUMMARY:summary,
  }});
  assert.equal(result.status,0,result.stderr);
  assert.match(fs.readFileSync(summary,'utf8'),/diagnostic scope: fast; this run does not qualify the SHA/);
 }finally{fs.rmSync(directory,{recursive:true,force:true});}
});

test('fast CLI runs manifest and parsing contracts while the normal tier retains its full CLI check',()=>{
 const step=fast.jobs['cli-tests'].steps.find(step=>step.name==='CLI tests');
 assert.match(step.run,/cargo test --manifest-path cli\/Cargo.toml --locked -p codeterm-cli cmd::manifest/);
 assert.match(step.run,/cargo test --manifest-path cli\/Cargo.toml --locked -p codeterm-cli plugin_subcommands_parse/);
 assert.match(step.run,/else\s+bash build.sh ci-cli-tests/);
 assert.equal(evaluateCondition(step.env.CI_FAST_DISPATCH,context('refs/heads/ci/try-cleanup')),true);
});
