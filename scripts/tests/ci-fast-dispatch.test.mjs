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
const ci=read('tests'),fast=read('tests-fast'),rust=read('tests-rust');
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

