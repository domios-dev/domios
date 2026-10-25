import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { condition } from './workflow-condition.mjs';

const directory = new URL('../../.github/workflows/', import.meta.url);
const workflows = Object.fromEntries(readdirSync(directory).filter(name => name.endsWith('.yml'))
  .map(name => [name, parse(readFileSync(new URL(name, directory), 'utf8'))]));
const github = { repository: 'rollacode/codeterm', actor: 'rollacode', triggering_actor: 'rollacode', event_name: 'push', ref: 'refs/heads/release/9.9.9' };
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

test('every PR entry point and reusable descendant is hosted, read-only and secret-free', () => {
  let entries = 0;
  for (const [name, workflow] of Object.entries(workflows)) {
    assert.equal(workflow.on.pull_request_target, undefined, name);
    if ('pull_request' in workflow.on) {
      entries++;
      assertPublicJobs(workflows, name);
    }
  }
  assert.ok(entries >= 7);
  assert.equal(workflows['tests.yml'].on.pull_request, undefined);
});

test('PR graph contract rejects nested self-hosted labels, secrets and writable tokens', () => {
  const root = { on: { pull_request: null }, permissions: { contents: 'read' }, jobs: { check: { uses: './.github/workflows/child.yml' } } };
  const leaf = { on: { workflow_call: null }, jobs: { check: { 'runs-on': 'ubuntu-24.04', steps: [{ run: 'true' }] } } };
  assertPublicJobs({ 'root.yml': root, 'child.yml': leaf }, 'root.yml');
  for (const mutation of [
    { 'runs-on': ['self-hosted', 'linux-build'] },
    { env: { TOKEN: '${{ secrets.RELEASE_TOKEN }}' } },
    { permissions: { contents: 'write' } },
    { secrets: 'inherit' },
  ]) {
    const child = structuredClone(leaf);
    Object.assign(child.jobs.check, mutation);
    assert.throws(() => assertPublicJobs({ 'root.yml': root, 'child.yml': child }, 'root.yml'));
  }
});

test('privileged reusable jobs refuse every PR actor, even the owner and same-repository PRs', () => {
  let guarded = 0;
  for (const [name, workflow] of Object.entries(workflows)) {
    for (const [id, job] of Object.entries(workflow.jobs)) {
      if (!job['runs-on']) continue;
      if (!stringify(job['runs-on']).includes('self-hosted') && !secrets(job) && !releaseWorkflow(name)) continue;
      guarded++;
      assert.match(job.if, /github\.repository == 'rollacode\/codeterm'/, `${name}/${id}`);
      assert.match(job.if, /github\.actor == 'rollacode'/, `${name}/${id}`);
      for (const event_name of ['pull_request', 'pull_request_target']) {
        for (const actor of ['rollacode', 'outside-collaborator', 'fork-contributor']) {
          assert.equal(condition(job.if, { github: { ...github, event_name, actor } }), false, `${name}/${id}/${actor}/${event_name}`);
        }
      }
      assert.equal(condition(job.if, { github: { ...github, actor: 'outside-collaborator' } }), false, `${name}/${id}: outside push`);
    }
  }
  assert.ok(guarded > 30);
});

test('release jobs gate runner acquisition and secrets on owner, release branch and allowed event', () => {
  for (const [name, workflow] of Object.entries(workflows).filter(([name]) => releaseWorkflow(name))) {
    for (const trigger of Object.keys(workflow.on)) assert.ok(['push', 'workflow_dispatch', 'workflow_call'].includes(trigger), `${name}/${trigger}`);
    if (workflow.on.push) assert.deepEqual(workflow.on.push, { branches: ['release/**'] });
    for (const [id, job] of Object.entries(workflow.jobs)) {
      if (job.uses) continue;
      assert.equal(job.environment, 'release', `${name}/${id}`);
      for (const invalid of [
        { actor: 'outside-collaborator' }, { triggering_actor: 'outside-collaborator' }, { repository: 'fork/codeterm' },
        { event_name: 'workflow_run' }, { event_name: 'schedule' },
        { ref: 'refs/heads/master' }, { ref: 'refs/tags/v9.9.9' },
      ]) assert.equal(condition(job.if, { github: { ...github, ...invalid } }), false, `${name}/${id}/${stringify(invalid)}`);
    }
  }
  for (const event_name of ['push', 'workflow_dispatch']) {
    assert.equal(condition(workflows['release.yml'].jobs.prepare.if, { github: { ...github, event_name } }), true);
  }
  assert.equal(condition(workflows['plugin-release.yml'].jobs.publish.if, { github: { ...github, event_name: 'workflow_dispatch' } }), true);
});

test('all secret references resolve inside a protected job environment', () => {
  for (const [name, workflow] of Object.entries(workflows)) {
    assert.ok(!secrets(workflow.env), `${name}: secrets must resolve after job environment approval`);
    for (const [id, job] of Object.entries(workflow.jobs)) {
      if (job.uses || !secrets(job)) continue;
      if (releaseWorkflow(name)) assert.equal(job.environment, 'release', `${name}/${id}`);
      else if (name === 'rust-warnings-summary.yml') {
        assert.equal(job.environment, "${{ startsWith(github.ref, 'refs/heads/release/') && 'release' || 'ci' }}");
      } else assert.equal(job.environment, 'ci', `${name}/${id}`);
    }
  }
});

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
