import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { historyArguments, fetchHistory, HISTORY_PURPOSES } from '../ci-git-history.mjs';
import { fetchWarningBase } from '../ci-warning-base.mjs';

const sha = 'a'.repeat(40), base = 'b'.repeat(40);
const workflows = new URL('../../.github/workflows/', import.meta.url);
const read = file => parse(fs.readFileSync(new URL(file, workflows), 'utf8'));
const checkoutErrors = step => {
  if (!step.uses?.startsWith('actions/checkout@')) return [];
  const errors = [];
  if (![1, 2].includes(step.with?.['fetch-depth'])) errors.push('checkout must fetch one commit (two for parent comparison)');
  if (!step.with?.ref) errors.push('checkout must select an explicit source');
  if (step.with?.['fetch-tags'] === true) errors.push('checkout cannot fetch all tags');
  return errors;
};

test('all workflow checkouts use bounded depth and explicit source; unrestricted history has no allowlist', () => {
  let count = 0;
  for (const file of fs.readdirSync(workflows).filter(file => file.endsWith('.yml'))) {
    for (const [id, job] of Object.entries(read(file).jobs)) {
      for (const step of job.steps || []) {
        assert.deepEqual(checkoutErrors(step), [], file + '/' + id);
        if (step.uses?.startsWith('actions/checkout@')) count++;
        assert.doesNotMatch(step.run || '', /git\s+fetch[^\n]*(?:--all|\+?refs\/heads\/\*|\+?refs\/tags\/\*)/, file + '/' + id);
      }
    }
  }
  assert.ok(count > 30);
  for (const depth of [0, undefined, 100]) assert.ok(checkoutErrors({uses:'actions/checkout@fixture',with:{ref:sha,'fetch-depth':depth}}).length);
  assert.ok(checkoutErrors({uses:'actions/checkout@fixture',with:{'fetch-depth':1}}).length);
});

test('scoped history fetches only exact source and purpose-owned refs with explicit reasons', () => {
  for (const [purpose, policy] of Object.entries(HISTORY_PURPOSES)) {
    assert.ok(policy.reason.length > 20);
    const args = historyArguments(purpose, sha, true);
    assert.deepEqual(args.slice(0,6), ['fetch','--no-tags','--filter=blob:none','--unshallow','origin',sha]);
    assert.deepEqual(args.slice(6), policy.refs);
    assert.ok(!args.some(arg => /refs\/(?:heads|tags)\/\*:/.test(arg)));
    assert.ok(!historyArguments(purpose, sha, false).includes('--unshallow'));
  }
  assert.throws(() => historyArguments('unknown', sha, true));
  assert.throws(() => historyArguments('policy', '--all', true));
  assert.ok(historyArguments('policy', sha, true, { PR_BASE_SHA:base, PR_HEAD_SHA:'--all', PUSH_BEFORE:'0'.repeat(40) }).includes(base));
});

test('scoped history authenticates readback without exposing the token in git arguments', () => {
  const calls=[];
  fetchHistory('ancestry', {GH_TOKEN:'opaque-fixture'}, (program,args,options) => {
    assert.equal(program,'git');calls.push({args,options});
    if(args[1]==='HEAD') return sha;
    if(args[1]==='--is-shallow-repository') return 'true';
  });
  assert.ok(!JSON.stringify(calls.at(-1).args).includes('opaque-fixture'));
  assert.ok(calls.at(-1).options.env.GIT_CONFIG_VALUE_1.startsWith('AUTHORIZATION: basic '));
});

test('five-minute warning receipt fetches only the API-proven fork point at depth one', async () => {
  const job=read('rust-warnings-summary.yml').jobs.report;
  assert.equal(job['timeout-minutes'],5);
  assert.ok(job.steps.some(s=>s.run==='node scripts/ci-warning-base.mjs'));
  assert.ok(!job.steps.some(s=>s.run?.includes('ci-git-history.mjs')));
  const calls=[];
  const result=await fetchWarningBase({
    env:{GITHUB_REPOSITORY:'fixture/source',GH_TOKEN:'opaque-fixture'},
    request:async url=>{assert.ok(url.endsWith('/compare/develop...'+sha));return {ok:true,json:async()=>({merge_base_commit:{sha:base}})};},
    git:(_program,args)=>{calls.push(args);return sha;},
  });
  assert.equal(result,base);
  assert.deepEqual(calls.at(-1),['fetch','--no-tags','--depth=1','origin',base]);
  await assert.rejects(fetchWarningBase({env:{GITHUB_REPOSITORY:'fixture/source'},git:()=>sha,request:async()=>({ok:false,status:403})}),/HTTP 403/);
});

test('PR and trusted guards install pinned nextest before checking the archive contract prerequisites', () => {
  for(const file of ['tests-pr.yml','tests-fast.yml']){
    const steps=read(file).jobs.guards.steps;
    const install=steps.findIndex(s=>s.uses?.startsWith('taiki-e/install-action@')&&s.with?.tool==='cargo-nextest@0.9.148');
    const preflight=steps.findIndex(s=>s.name==='Runner preflight');
    assert.ok(install>=0&&install<preflight,file);
    assert.match(steps[install].uses,/@[a-f0-9]{40}$/);
    assert.ok(!steps[install].if || steps[install].if.includes("runner.environment == 'github-hosted'"));
  }
});
