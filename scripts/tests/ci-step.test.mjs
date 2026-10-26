import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {parseStepArgs,runStep,catalogueEntry} from '../ci-step.mjs';
import {runnerProfiles,profileEnvironment,inferRunnerProfile} from '../ci-runner-profile.mjs';
import {stepBindings} from '../generate-ci-step-contracts.mjs';
import {reproLine} from '../ci-repro.mjs';

const root=path.resolve(import.meta.dirname,'../..');
test('failure commands include the exact source, runner profile and focused selection',()=>{
 const sha='a'.repeat(40);
 assert.equal(reproLine('node-script-tests',{profile:'hosted-linux',sha,file:'scripts/tests/fixture.test.mjs'}),
  'repro: node scripts/ci-step.mjs node-script-tests --as hosted-linux --source-sha '+sha+' --file scripts/tests/fixture.test.mjs');
 assert.deepEqual(parseStepArgs(['node-script-tests','--as','hosted-linux','--source-sha',sha,'--file','fixture.mjs']),
  {step:'node-script-tests',profile:'hosted-linux',sha,file:'fixture.mjs'});
 assert.throws(()=>parseStepArgs(['node-script-tests','--typo']),/Invalid ci-step option/);
});
