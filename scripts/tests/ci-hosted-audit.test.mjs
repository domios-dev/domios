import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {parse} from 'yaml';
import {bashInterpreter} from '../ci-runner-paths.mjs';
import {sweepRustTargets} from '../ci-workspace-cleanup.mjs';
const workflow=name=>parse(fs.readFileSync(new URL('../../.github/workflows/'+name+'.yml',import.meta.url),'utf8'));
test('all frontend consumers download the producer digest rather than the consumer attempt',()=>{
 for(const name of ['release-arm64-compile','release-linux','release-macos-build','release-windows']){
  const steps=Object.values(workflow(name).jobs)[0].steps;
  const download=steps.find(step=>step.name==='Download digest-pinned frontend bundle');
  assert.equal(download.with.name,'frontend-dist-'+'$'+'{{ inputs.frontend_sha256 }}');
  assert.ok(steps.some(step=>/release-frontend-dist.mjs fetch-run-artifact/.test(step.run||'')));
 }
 for(const name of ['linux','windows']){
  const job=workflow('release-'+name+'-arm64').jobs.build;
  const download=job.steps.find(step=>step.uses?.startsWith('actions/download-artifact@'));
  assert.ok(download.with.name.includes('needs.compile.outputs.sha256'));
  assert.ok(!download.with.name.includes('run_attempt'));
 }
 const steps=workflow('release-arm64-compile').jobs.compile.steps;
 assert.ok(steps.findIndex(step=>step.name==='Prepare pinned toolchain')<steps.findIndex(step=>step.name==='Runner preflight'));
 assert.doesNotMatch(steps.find(step=>step.name==='Runner preflight').run,/\bgh\b/);
 assert.equal(workflow('release-arm64-compile').jobs.compile.environment,undefined);
});
test('plugin publication fetches only the validated tag before resolving it',()=>{
 const step=workflow('plugin-release').jobs.publish.steps.find(step=>step.name==='Require a verified master commit and exact plugin tag');
 assert.ok(step.run.indexOf('[[ "$PLUGIN_TAG"')<step.run.indexOf('git fetch --no-tags --depth=1'));
 assert.ok(step.run.indexOf('git fetch --no-tags --depth=1')<step.run.indexOf('git rev-parse'));
 assert.match(step.run,/refs\/tags\/\$PLUGIN_TAG:refs\/tags\/\$PLUGIN_TAG/);
});
test('disposable hosted targets never invoke cargo sweep',()=>{
 assert.deepEqual(sweepRustTargets('absent',()=>{throw new Error('must not invoke');},undefined,{RUNNER_ENVIRONMENT:'github-hosted'}),[]);
});
test('warning transport shares discovery of alternate Git installations',()=>{
 assert.equal(bashInterpreter('win32',{PATH:'D:\\PortableGit\\cmd'},file=>file==='D:\\PortableGit\\bin\\bash.exe'),'D:\\PortableGit\\bin\\bash.exe');
});
test('spool reads merge siblings, accept identical duplicates and reject conflicting keys',()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'ci-spool-union-'));
 try{
  const temp=path.join(root,'tmp');
  for(const [job,key,value] of [['full','full.json','one'],['mac','mac.json','two'],['windows','windows.json','three'],['embeddings','embeddings.json','four']]){
   const dir=path.join(temp,'ci-artifact-fallback','41',job,'41','rust-receipt');fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,key),value);
  }
  const run=()=>spawnSync(bashInterpreter(),['scripts/ci-artifact.sh','get-prefix','41/rust-receipt/',path.join(root,'output').replaceAll('\\','/')],{encoding:'utf8',env:{...process.env,GITHUB_ACTIONS:'true',RUNNER_TEMP:temp.replaceAll('\\','/'),SCCACHE_ENDPOINT:'http://127.0.0.1:9'}});
  assert.equal(run().status,0);
  assert.equal(fs.readdirSync(path.join(root,'output')).length,4);
  const duplicate=path.join(temp,'ci-artifact-fallback','41','duplicate','41','rust-receipt');fs.mkdirSync(duplicate,{recursive:true});fs.writeFileSync(path.join(duplicate,'full.json'),'one');
  assert.equal(run().status,0);
  fs.writeFileSync(path.join(duplicate,'full.json'),'conflict');const conflict=run();assert.notEqual(conflict.status,0);assert.match(conflict.stderr,/conflicting copies/);
 }finally{fs.rmSync(root,{recursive:true,force:true});}
});
test('Actions receipts always spool even if private-store credentials exist',()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'ci-backend-'));
 try{
  const input=path.join(root,'input');fs.writeFileSync(input,'receipt');
  const result=spawnSync(bashInterpreter(),['scripts/ci-artifact.sh','put','41/rust-receipt/full.json',input.replaceAll('\\','/')],{encoding:'utf8',env:{...process.env,GITHUB_ACTIONS:'true',RUNNER_TEMP:root.replaceAll('\\','/'),SCCACHE_ENDPOINT:'http://127.0.0.1:9',AWS_ACCESS_KEY_ID:'fixture',AWS_SECRET_ACCESS_KEY:'fixture'}});
  assert.equal(result.status,0,result.stderr);
  assert.equal(fs.readFileSync(path.join(root,'ci-artifact-spool','41','rust-receipt','full.json'),'utf8'),'receipt');
 }finally{fs.rmSync(root,{recursive:true,force:true});}
});

test('credential API checks use the action-authenticated clients without requiring action-private packages',async()=>{
 const steps=workflow('release-credentials').jobs.linux.steps;
 const checks=steps.filter(step=>step.name==='Updater signing key and repository write access'||step.name==='Mirror publication token write access');
 const calls=[];
 for(const step of checks){
  assert.ok(step.with['github-token'].includes('secrets.'));
  const execute=new (Object.getPrototypeOf(async function(){}).constructor)('process','Buffer','github','core',step.with.script);
  await execute({env:{GITHUB_REPOSITORY:'fixture/source',TAURI_SIGNING_PRIVATE_KEY:Buffer.from('untrusted comment: opaque').toString('base64')}},
   Buffer,{rest:{repos:{get:async args=>{calls.push(args);return{data:{permissions:{push:true}}};}}}},{info:()=>{}});
 }
 assert.deepEqual(calls,[{owner:'fixture',repo:'source'},{owner:'rollacode',repo:'codeterm-releases'},{owner:'rollacode',repo:'codeterm-releases'}]);
});
test('small evidence has short retention and large release evidence keeps the private path',()=>{
 const spool=parse(fs.readFileSync(new URL('../../.github/actions/ci-artifact-spool/action.yml',import.meta.url),'utf8'));
 assert.equal(spool.runs.steps.find(step=>step.uses?.startsWith('actions/upload-artifact@')).with['retention-days'],3);
 for(const step of workflow('release-arm64-compile').jobs.compile.steps.filter(step=>step.uses?.startsWith('actions/upload-artifact@')))assert.equal(step.with['retention-days'],1);
 const frontend=workflow('release').jobs['frontend-dist'].steps.find(step=>step.uses?.startsWith('actions/upload-artifact@'));
 assert.equal(frontend.with['retention-days'],1);
 const transport=fs.readFileSync(new URL('../ci-artifact.sh',import.meta.url),'utf8');
 const policy=transport.slice(transport.indexOf('store_down()'),transport.indexOf('spool_put()'));
 assert.doesNotMatch(policy,/release-dry-run|frontend-release/);
});
