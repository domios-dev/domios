import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';
import path from 'node:path';
import { createRequire } from 'node:module';
import { runInNewContext } from 'node:vm';
import { parse } from 'yaml';
import { runnerRequirements } from '../ci-runner-paths.mjs';
import { condition } from './workflow-condition.mjs';
import { runnerKinds } from './ci-runner-scenarios.mjs';

const providers = { node:'actions/setup-node@',python:'actions/setup-python@',pnpm:'pnpm/action-setup@' };
const installer = step => /^(actions\/setup-|pnpm\/action-setup@|dtolnay\/rust-toolchain@|taiki-e\/install-action@|mozilla-actions\/sccache-action@)/.test(step.uses||'');
const pinned = step => /@[a-f0-9]{40}$/.test(step.uses||'') && !step['continue-on-error'];
function activeProvider(step,kind){
  if(!step.if)return true;
  if(!step.if.includes('runner.environment'))return false;
  return condition(step.if,{runner:{environment:kind},matrix:{},vars:{},steps:{}});
}
function orderingErrors(steps,kind='github-hosted'){
  const ready=new Set(),errors=[];
  for(const step of steps){
    const provider=Object.keys(providers).find(tool=>step.uses?.startsWith(providers[tool]));
    if(provider){
      if(provider==='pnpm' && activeProvider(step,kind) && !ready.has('node'))errors.push('pnpm installer used before Node');
      if(pinned(step)&&activeProvider(step,kind))ready.add(provider);
      continue;
    }
    if(step.uses?.startsWith('actions/github-script@')&&pinned(step)&&activeProvider(step,kind)){
      assert.match(step.with.script,/core\.addPath\(path\.dirname\(process\.execPath\)\)/);
      ready.add('node');
    }
    if(step.if?.includes("runner.environment == 'self-hosted'")&&kind==='github-hosted')continue;
    const command=step.run||'';
    const required=new Set([...command.matchAll(/\b(node|pnpm|python3?)\b/g)].map(m=>m[1].startsWith('python')?'python':m[1]));
    for(const [,role='rust']of command.matchAll(/node\s+scripts\/ci-runner-paths\.mjs(?:\s+([^\s]+))?/g)){
      if(role!=='--paths-only')for(const tool of runnerRequirements(role))if(tool in providers)required.add(tool);
    }
    for(const tool of required)if(!ready.has(tool))errors.push(tool+' used before provisioning/discovery: '+(step.name||command));
    if(kind==='self-hosted'&&command==='node scripts/ci-runner-paths.mjs --paths-only'){
      ready.add('python');ready.add('pnpm');
    }
  }
  return errors;
}

test('every workflow provisions hosted tools or discovers installed self-hosted tools before scripts',()=>{
  const directory=new URL('../../.github/workflows/',import.meta.url);
  for(const file of readdirSync(directory).filter(file=>file.endsWith('.yml'))){
    for(const [id,job]of Object.entries(parse(readFileSync(new URL(file,directory),'utf8')).jobs)){
      if(!job.steps)continue;
      const kinds=runnerKinds(job);
      for(const kind of kinds)assert.deepEqual(orderingErrors(job.steps,kind),[],file+'/'+id+'/'+kind);
    }
  }
});

test('no installer action can execute on a self-hosted runner',()=>{
  const directory=new URL('../../.github/workflows/',import.meta.url);
  let checked=0;
  for(const file of readdirSync(directory).filter(file=>file.endsWith('.yml'))){
    for(const [id,job]of Object.entries(parse(readFileSync(new URL(file,directory),'utf8')).jobs)){
      if(!job.steps || !runnerKinds(job).includes('self-hosted'))continue;
      for(const step of job.steps.filter(installer)){
        assert.equal(condition(step.if,{runner:{environment:'self-hosted'},matrix:{},vars:{},steps:{}}),false,file+'/'+id+'/'+step.uses);
        checked++;
      }
    }
  }
  assert.ok(checked>30);
});

test('negative controls refuse early Node, Python and pnpm and an installer before discovery',()=>{
  const setup=tool=>({uses:providers[tool]+'a'.repeat(40)});
  for(const tool of Object.keys(providers))assert.ok(orderingErrors([{run:tool+' fixture'}]).length);
  assert.deepEqual(orderingErrors([setup('node'),setup('python'),setup('pnpm'),{run:'node scripts/ci-runner-paths.mjs pr-guards'}]),[]);
  assert.ok(orderingErrors([{...setup('node'),if:'steps.tree.outputs.run'},{run:'node fixture'}]).length);
  assert.ok(orderingErrors([{run:'python fixture.py'},{run:'node scripts/ci-runner-paths.mjs --paths-only'}],'self-hosted').length);
});

test('runner-owned Node bootstrap works without a system Node or shell PATH',()=>{
  const workflow=parse(readFileSync(new URL('../../.github/workflows/tests-fast.yml',import.meta.url),'utf8'));
  const steps=workflow.jobs.changed.steps;
  const bootstrap=steps.find(step=>step.uses?.startsWith('actions/github-script@'));
  const executable=path.resolve('opaque-runner','externals','node24','bin',process.platform==='win32'?'node.exe':'node');
  const added=[];
  runInNewContext(bootstrap.with.script,{
    require:createRequire(import.meta.url),process:{execPath:executable,version:'v24.fixture',env:{}},
    core:{addPath:directory=>added.push(directory),info:()=>{}},
  });
  assert.deepEqual(added,[path.dirname(executable)]);
  assert.ok(orderingErrors(steps.filter(step=>step!==bootstrap),'self-hosted').some(error=>error.startsWith('node used before')));
});
