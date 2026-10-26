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
test('failure commands include the exact source, runner profile and focused selection',()=>{
 const sha='a'.repeat(40);
 assert.equal(reproLine('node-script-tests',{profile:'hosted-linux',sha,file:'scripts/tests/fixture.test.mjs'}),
  'repro: node scripts/ci-step.mjs node-script-tests --as hosted-linux --source-sha '+sha+' --file scripts/tests/fixture.test.mjs');
 assert.deepEqual(parseStepArgs(['node-script-tests','--as','hosted-linux','--source-sha',sha,'--file','fixture.mjs']),
  {step:'node-script-tests',profile:'hosted-linux',sha,file:'fixture.mjs'});
 assert.throws(()=>parseStepArgs(['node-script-tests','--typo']),/Invalid ci-step option/);
});
