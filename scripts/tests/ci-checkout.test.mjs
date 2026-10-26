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


test('scoped history fetches only exact source and purpose-owned refs with explicit reasons', () => {
  for (const [purpose, policy] of Object.entries(HISTORY_PURPOSES)) {
    assert.ok(policy.reason.length > 20);
    const args = historyArguments(purpose, sha, true);
    assert.deepEqual(args.slice(0,6), ['fetch','--no-tags','--filter=blob:none','--unshallow','origin',sha]);
    // Stale tracking refs on persistent self-hosted checkouts must not refuse the fetch.
    assert.deepEqual(args.slice(6), policy.refs.map(ref => '+' + ref));
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


