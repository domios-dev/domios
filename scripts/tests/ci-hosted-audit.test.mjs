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

