import assert from 'node:assert/strict';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { parse } from 'yaml';
import { jobTools, commandTools } from '../ci-job-tools.mjs';
import { condition } from './workflow-condition.mjs';
import { runnerKinds } from './ci-runner-scenarios.mjs';
import { runnerRequirements, checkRunnerTools } from '../ci-runner-paths.mjs';

const read = name => parse(fs.readFileSync(new URL('../../.github/workflows/' + name + '.yml', import.meta.url), 'utf8'));
const files = ['tests', 'tests-fast', 'tests-rust', 'tests-pr'];
function provisionErrors(job, kind='github-hosted') {
  const errors = [];
  const steps = job.steps;
  const preflight = steps.findIndex(step => step.name === 'Runner preflight');
  if (preflight < 0) return ['missing preflight'];
  const required = jobTools(job);
  const declared = runnerRequirements(steps[preflight].run.split(/\s+/).at(-1));
  if (JSON.stringify(required) !== JSON.stringify(declared)) errors.push('declared tools differ from command consumers');
  const before = steps.slice(0, preflight);
  const enabled = step => !step.if || (step.if.includes('runner.environment') && condition(step.if,{runner:{environment:kind}}));
  const discovered = kind==='self-hosted' && before.some(step=>step.run==='node scripts/ci-runner-paths.mjs --paths-only' && enabled(step));
  const pinned = prefix => before.some(step => step.uses?.startsWith(prefix) && /@[a-f0-9]{40}$/.test(step.uses) && enabled(step));
  for (const tool of ['node', 'python', 'pnpm']) {
    const prefix = tool === 'pnpm' ? 'pnpm/action-setup@' : 'actions/setup-' + tool + '@';
    if (required.includes(tool) && !pinned(prefix) && !(kind==='self-hosted' && (tool==='node' ? pinned('actions/github-script@') : discovered))) errors.push(tool + ' not provisioned before preflight');
  }
  const nextest = before.find(step => step.uses?.startsWith('taiki-e/install-action@') && step.with?.tool === 'cargo-nextest@0.9.148' && enabled(step));
  if (required.includes('nextest') && !nextest && kind==='github-hosted') errors.push('nextest not installed before preflight');
  if (!required.includes('nextest') && steps.some(step => step.with?.tool?.startsWith('cargo-nextest@'))) errors.push('unused nextest installation');
  if (required.includes('nextest') && kind==='github-hosted') {
    const installedAt = steps.indexOf(nextest);
    if (steps.some((step,index) => commandTools(step.run || '').includes('nextest') && index < installedAt)) errors.push('nextest invoked before installation');
  }
  if (required.includes('python:yaml') && !before.some(step =>
    step.run === 'python -m pip install --disable-pip-version-check -r scripts/requirements-ci.txt' && !step.if)) errors.push('Python modules not installed before preflight');
  return errors;
}

test('simulate every CI executable job against its command-derived tool and Python-module inventory', () => {
  let count = 0;
  for (const name of files) for (const [id,job] of Object.entries(read(name).jobs)) {
    if (!job.steps) continue;
    const kinds=runnerKinds(job);
    for(const kind of kinds)assert.deepEqual(provisionErrors(job,kind), [], name + '/' + id+'/'+kind);
    const probes=[];
    checkRunnerTools(job.steps.find(step => step.name==='Runner preflight').run.split(/\s+/).at(-1), {
      platform:'linux', execute:(command,args)=>{probes.push({command,args});return {status:0};},
    });
    assert.equal(probes.length,jobTools(job).length,name+'/'+id);
    console.log(name+'/'+id+' ['+kinds.join('|')+']: '+jobTools(job).join(','));
    count++;
  }
  assert.equal(count,30);
});

test('negative controls reject stale role, missing nextest provider, and missing Python dependencies', () => {
  const types=structuredClone(read('tests-fast').jobs['ts-types']);
  assert.ok(!jobTools(types).includes('nextest'));
  types.steps.find(step=>step.name==='Runner preflight').run='node scripts/ci-runner-paths.mjs pr-guards';
  assert.ok(provisionErrors(types).length);
  const rust=structuredClone(read('tests-rust').jobs['rust-tests']);
  rust.steps=rust.steps.filter(step=>!step.with?.tool?.startsWith('cargo-nextest@'));
  assert.ok(provisionErrors(rust).some(error=>error.includes('nextest')));
  const early=structuredClone(read('tests-rust').jobs['rust-tests']);
  early.steps.unshift({run:'cargo nextest run --workspace'});
  assert.ok(provisionErrors(early).some(error=>error.includes('before installation')));
  const guards=structuredClone(read('tests-pr').jobs.guards);
  guards.steps=guards.steps.filter(step=>!step.run?.includes('pip install'));
  assert.ok(provisionErrors(guards).some(error=>error.includes('Python modules')));
});

test('entrypoint semantics distinguish compilation, Cargo tests, nextest execution and receipt verification', () => {
  assert.ok(!commandTools('bash build.sh ci-clippy').includes('nextest'));
  assert.ok(!commandTools('bash build.sh ci-cli-tests').includes('nextest'));
  assert.ok(!commandTools('pnpm types:check').includes('nextest'));
  assert.ok(commandTools('pnpm types:check').includes('cargo'));
  assert.ok(commandTools('bash build.sh ci-tests platform').includes('nextest'));
  assert.ok(commandTools('cargo nextest run --workspace').includes('nextest'));
  assert.ok(!commandTools('node scripts/ci-rust-tests.mjs verify ci-rust-receipts').includes('nextest'));
});

test('Python test AST inventory is covered by pinned requirements and module probes', () => {
  const python=process.platform==='win32'?'python':'python3';
  const result=spawnSync(python,['scripts/ci-python-imports.py'],{encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  assert.deepEqual(JSON.parse(result.stdout),['yaml']);
  assert.match(fs.readFileSync(new URL('../requirements-ci.txt',import.meta.url),'utf8').trim(),/^PyYAML==\d+\.\d+\.\d+$/);
  assert.throws(()=>checkRunnerTools('python,python:yaml',{platform:'linux',execute:(_command,args)=>({status:args.includes('-c')?1:0})}),/python:yaml.*requirements-ci\.txt/);
  assert.throws(()=>runnerRequirements('python:yaml;exit'),/Unknown runner role/);
});
