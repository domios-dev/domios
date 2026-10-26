import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import {parse} from 'yaml';
import {condition} from './workflow-condition.mjs';
import {ciFailures} from '../ci-result.mjs';
import {jobTools} from '../ci-job-tools.mjs';
import {runnerScenarios} from './ci-runner-scenarios.mjs';
import { preparePython } from '../ci-python-env.mjs';
import { pythonInterpreter } from '../ci-runner-paths.mjs';
import { catalogueCommand, runCatalogue } from '../ci-catalogue-command.mjs';

test('interpreter discovery accepts Linux python3 without a python alias and preserves the exported venv',()=>{
  assert.equal(pythonInterpreter('linux',{PATH:'/usr/bin:/bin'},file=>file==='/usr/bin/python3'),'/usr/bin/python3');
  assert.equal(pythonInterpreter('darwin',{PATH:'/opt/homebrew/bin'},file=>file==='/opt/homebrew/bin/python3'),'/opt/homebrew/bin/python3');
  assert.equal(pythonInterpreter('win32',{PATH:'C:\\Python312'},file=>file==='C:\\Python312\\python.exe'),'C:\\Python312\\python.exe');
  assert.equal(pythonInterpreter('linux',{CI_PYTHON:'/job/venv/bin/python3'}),'/job/venv/bin/python3');
});


test('missing catalogue, malformed selection and absent entries have named errors before execution',()=>{
  for(const read of [()=>{throw new Error('ENOENT');},()=>'',()=>'{broken',()=>JSON.stringify({runners:[]})]){
    assert.throws(()=>catalogueCommand('fixture',{directory:'/job',read}),/CI catalogue (?:unavailable|entry unavailable)/);
  }
  const command=catalogueCommand('fixture',{directory:'/job',read:()=>JSON.stringify({runners:[{id:'fixture',runner:'echo success'}]})});
  assert.equal(command,'echo success');
  assert.equal(runCatalogue('fixture',{env:{RUNNER_TEMP:'/job',CI_PYTHON:'/job/python3'},read:()=>JSON.stringify({runners:[{id:'fixture',runner:'echo success'}]}),execute:(_program,args,options)=>{
    assert.deepEqual(args,['-c','echo success']);assert.equal(options.env.CI_PYTHON,'/job/python3');return{status:37};
  }}),37);
});

test('catalogue CLI emits a named failure instead of a workflow template error',()=>{
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'ci-catalogue-missing-'));
  try{
    const result=spawnSync(process.execPath,['scripts/ci-catalogue-command.mjs','fixture'],{env:{...process.env,RUNNER_TEMP:directory},encoding:'utf8'});
    assert.equal(result.status,1);
    assert.match(result.stderr,/::error title=CI catalogue unavailable::/);
  }finally{fs.rmSync(directory,{recursive:true,force:true});}
});




