import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';
import path from 'node:path';
import { createRequire } from 'node:module';
import { runInNewContext } from 'node:vm';
import { parse } from 'yaml';
import { runnerRequirements } from '../ci-runner-paths.mjs';
import { condition } from './workflow-condition.mjs';
import { runnerKinds, runnerScenarios } from './ci-runner-scenarios.mjs';

const providers = { node:'actions/setup-node@',python:'actions/setup-python@',pnpm:'pnpm/action-setup@' };
const installer = step => /^(actions\/setup-|pnpm\/action-setup@|dtolnay\/rust-toolchain@|taiki-e\/install-action@|mozilla-actions\/sccache-action@)/.test(step.uses||'');
const pinned = step => /@[a-f0-9]{40}$/.test(step.uses||'') && !step['continue-on-error'];
function activeProvider(step,kind,matrix={}){
  if(!step.if)return true;
  if(!step.if.includes('runner.environment')&&!step.if.includes('matrix.'))return false;
  return condition(step.if,{runner:{environment:kind},matrix,vars:{},steps:{}});
}
function orderingErrors(steps,kind='github-hosted',matrix={}){
  const ready=new Set(),errors=[];
  for(const step of steps){
    if(step.if?.includes('matrix.')&&!activeProvider(step,kind,matrix))continue;
    const provider=Object.keys(providers).find(tool=>step.uses?.startsWith(providers[tool]));
    if(provider){
      if(provider==='pnpm' && activeProvider(step,kind,matrix) && !ready.has('node'))errors.push('pnpm installer used before Node');
      if(pinned(step)&&activeProvider(step,kind,matrix))ready.add(provider);
      continue;
    }
    if(step.uses?.startsWith('actions/github-script@')&&pinned(step)&&activeProvider(step,kind,matrix)&&
        /core\.addPath\(path\.dirname\(process\.execPath\)\)/.test(step.with.script)){
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



test('negative controls refuse early Node, Python and pnpm and an installer before discovery',()=>{
  const setup=tool=>({uses:providers[tool]+'a'.repeat(40)});
  for(const tool of Object.keys(providers))assert.ok(orderingErrors([{run:tool+' fixture'}]).length);
  assert.deepEqual(orderingErrors([setup('node'),setup('python'),setup('pnpm'),{run:'node scripts/ci-runner-paths.mjs pr-guards'}]),[]);
  assert.ok(orderingErrors([{...setup('node'),if:'steps.tree.outputs.run'},{run:'node fixture'}]).length);
  assert.ok(orderingErrors([{run:'python fixture.py'},{run:'node scripts/ci-runner-paths.mjs --paths-only'}],'self-hosted').length);
});

