import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { condition, evaluateCondition } from './workflow-condition.mjs';

const directory = new URL('../../.github/workflows/', import.meta.url);
const workflows = Object.fromEntries(readdirSync(directory).filter(name => name.endsWith('.yml'))
  .map(name => [name, parse(readFileSync(new URL(name, directory), 'utf8'))]));
const github = { repository: 'rollacode/codeterm', repository_owner: 'rollacode', actor: 'rollacode', triggering_actor: 'rollacode', event_name: 'push', ref: 'refs/heads/release/9.9.9', event: { workflow_run: {} } };
const hostedCompile = (name, id) => name === 'release.yml' && id === 'qualified-source';
const releaseWorkflow = name => /^(release|frontend-release|plugin-release|publish-plugin-sdk)/.test(name);
const stringify = value => JSON.stringify(value ?? {});
const secrets = value => /\bsecrets\./.test(stringify(value));

function assertReadOnly(permissions) {
  assert.notEqual(permissions, 'write-all');
  assert.ok(Object.values(permissions ?? {}).every(value => value !== 'write'));
}

function assertPublicJobs(graph, name, visited = new Set()) {
  assert.ok(!visited.has(name), `recursive workflow: ${name}`);
  visited = new Set([...visited, name]);
  const workflow = graph[name];
  assert.ok(workflow, `missing reusable workflow: ${name}`);
  assertReadOnly(workflow.permissions);
  assert.ok(!secrets(workflow.env), `${name}: workflow secrets`);
  for (const [id, job] of Object.entries(workflow.jobs)) {
    if (job.if?.includes('github.event_name') && !condition(job.if, {
      github: { ...github, event_name: 'pull_request' }, inputs: { source_ref: '' },
      needs: { fast: { outputs: {} } },
    })) continue;
    assertReadOnly(job.permissions);
    assert.equal(job.secrets, undefined, `${name}/${id}: inherited secrets on PR path`);
    assert.ok(!secrets(job), `${name}/${id}: secrets on PR path`);
    assert.equal(job.environment, undefined, `${name}/${id}: environment on PR path`);
    if (job.uses) {
      assert.match(job.uses, /^\.\/\.github\/workflows\/[^/]+\.yml$/);
      assertPublicJobs(graph, job.uses.split('/').at(-1), visited);
    } else {
      assert.match(job['runs-on'], /^(ubuntu|windows|macos)-[\w.-]+$/, `${name}/${id}: hosted literal required`);
      assert.ok(!secrets(workflow.defaults), `${name}: secrets in defaults`);
    }
  }
}






// Owner decision: only release jobs run in an environment; other jobs read repo-level secrets directly.

test('owner handoff preserves strict branch rules, protected environments and fork restrictions', () => {
  const admin = readFileSync(new URL('../../.github/admin/public-repo.sh', import.meta.url), 'utf8');
  for (const required of ['actor_type:"User"', 'type:"update"', 'type:"deletion"', 'type:"non_fast_forward"',
    'type:"pull_request"', 'require_code_owner_review:true', 'type:"required_status_checks"',
    'prevent_self_review:false', 'reviewers:[{type:"User",id:$owner}]', "name='release/*'", 'approval_policy=all_external_contributors',
    'send_write_tokens_to_workflows=false', 'send_secrets_and_variables=false']) assert.ok(admin.includes(required), required);
  const ownership = readFileSync(new URL('../../.github/CODEOWNERS', import.meta.url), 'utf8');
  for (const path of ['/.github/', '/scripts/release*', '/plugins/channel.json', '**/Cargo.toml', '**/package.json']) {
    assert.ok(ownership.includes(`${path} @rollacode`), path);
  }
});


