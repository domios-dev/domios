import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {parseStepArgs,runStep,catalogueEntry} from '../ci-step.mjs';
import {runnerProfiles,profileEnvironment,inferRunnerProfile} from '../ci-runner-profile.mjs';
import {stepBindings} from '../generate-ci-step-contracts.mjs';
import {reproLine} from '../ci-repro.mjs';
import {PREFLIGHT_PROFILES} from '../ci-preflight-profile.mjs';

const root=path.resolve(import.meta.dirname,'../..');
test('one profile file defines every replay environment and every live preflight profile',()=>{
 assert.deepEqual(Object.keys(runnerProfiles.profiles).sort(),['hosted-linux','hosted-macos','hosted-windows','mac-service','rog-service','ubunta-service']);
 assert.deepEqual(PREFLIGHT_PROFILES,Object.fromEntries(Object.entries(runnerProfiles.preflight).map(([id,row])=>[id,row.tools])));
 for(const[id,profile]of Object.entries(runnerProfiles.profiles)){
  const env=profileEnvironment(id,{PATH:'opaque-warm-path',CARGO_HOME:'warm',CI_PYTHON:'warm-python',AWS_SECRET_ACCESS_KEY:'opaque',RUSTC_WRAPPER:'warm'},'/scratch',{platform:'linux',toolPaths:['/tools'],nodeDirectory:'/node'});
  assert.equal(env.RUNNER_ENVIRONMENT,profile.environment);assert.equal(env.CI,'true');assert.equal(env.CI_REPRO_PYTHON_COMMAND,profile.python);
  assert.equal(env.CARGO_HOME,path.join('/scratch','cargo-home'));assert.equal(env.CI_PYTHON,undefined);assert.equal(env.AWS_SECRET_ACCESS_KEY,undefined);
  assert.equal(env.RUSTC_WRAPPER,'');assert.ok(!env.PATH.includes('opaque-warm-path'));
 }
 assert.equal(inferRunnerProfile({RUNNER_ENVIRONMENT:'github-hosted'},'win32'),'hosted-windows');
 assert.equal(inferRunnerProfile({},'linux'),'ubunta-service');
 assert.throws(()=>parseStepArgs(['node-script-tests','--as','invented']),/Unknown runner profile/);
});
test('every workflow catalogue command has a reachable, fresh executor binding',()=>{
 const expected=stepBindings(root);
 assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root,'scripts/ci-step-contracts.json'),'utf8')),expected);
 assert.ok(expected.bindings.length>=30);
 for(const binding of expected.bindings){
  assert.equal(catalogueEntry(root,binding.entry).id,binding.entry);
  const source=fs.readFileSync(path.join(root,'.github/workflows',binding.file),'utf8');
  assert.doesNotMatch(source,/node scripts\/ci-catalogue-command\.mjs/);
  assert.ok(source.includes('ci-step.mjs '+binding.entry+' --ci'));
 }
});
test('failure commands include the exact source, runner profile and focused selection',()=>{
 const sha='a'.repeat(40);
 assert.equal(reproLine('node-script-tests',{profile:'hosted-linux',sha,file:'scripts/tests/fixture.test.mjs'}),
  'repro: node scripts/ci-step.mjs node-script-tests --as hosted-linux --source-sha '+sha+' --file scripts/tests/fixture.test.mjs');
 assert.deepEqual(parseStepArgs(['node-script-tests','--as','hosted-linux','--source-sha',sha,'--file','fixture.mjs']),
  {step:'node-script-tests',profile:'hosted-linux',sha,file:'fixture.mjs'});
 assert.throws(()=>parseStepArgs(['node-script-tests','--typo']),/Invalid ci-step option/);
});
test('the executor uses the selected CI command, preserves failure and prints repro last',async()=>{
 const directory=fs.mkdtempSync(path.join(os.tmpdir(),'ci-step-model-'));
 try{
  fs.mkdirSync(path.join(directory,'tests'));fs.mkdirSync(path.join(directory,'.github/workflows'),{recursive:true});
  const entry={id:'fixture-step',runner:'opaque-fixture-command',tier:'pr',budget_secs:3,discovery:{method:'single'}};
  fs.writeFileSync(path.join(directory,'tests/catalogue.json'),JSON.stringify({runners:[entry]}));
  fs.writeFileSync(path.join(directory,'pr-catalogue.json'),JSON.stringify({runners:[entry]}));
  const calls=[],output=[],sha='a'.repeat(40);
  const result=await runStep({step:entry.id,ci:true,profile:'hosted-linux',checkout:directory,sha},{
   base:{RUNNER_TEMP:directory},write:line=>output.push(line),
   execute:(program,args,options)=>{
    calls.push({program,args,options});
    if(program==='git'&&args[0]==='rev-parse')return{status:0,stdout:sha};
    if(program==='bash'&&args[0]==='-c'){assert.equal(args[1],entry.runner);assert.equal(options.timeout,3000);return{status:37};}
    return{status:0,stdout:''};
   },
  });
  assert.equal(result,37);assert.ok(calls.some(call=>call.args[1]===entry.runner));
  assert.ok(output.at(-1).startsWith('repro: node scripts/ci-step.mjs fixture-step --as hosted-linux --source-sha '+sha));
 }finally{fs.rmSync(directory,{recursive:true,force:true});}
});
