import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { parse } from "yaml";
import { condition } from "./workflow-condition.mjs";

function workflow(name) {
  return parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), "utf8"));
}

const release = workflow("release");
const tests = workflow("tests");
const fast = workflow("tests-fast");
const rust = workflow("tests-rust");
const builds = ["macos-arm64", "macos-x64", "linux-arm64", "linux-x86_64", "windows-x64", "windows-arm64"];
const dependencies = job => [job.needs].flat();

const qualifiedSha = 'a'.repeat(40);
const qualifiedGithub = {
  repository: 'rollacode/codeterm', repository_owner: 'rollacode', actor: 'rollacode', triggering_actor: 'rollacode',
  event_name: 'workflow_run', ref: 'refs/heads/master',
  event: { workflow_run: {
    conclusion: 'success', event: 'push', path: '.github/workflows/tests.yml', head_branch: 'release/9.9.9', head_sha: qualifiedSha,
    head_repository: { full_name: 'rollacode/codeterm' }, actor: { login: 'rollacode' }, triggering_actor: { login: 'rollacode' },
  } },
};

test('release packaging belongs to push CI and merge publication checks the release head', () => {
  assert.deepEqual(release.on.push,{branches:['master']});
  assert.equal(release.on.workflow_run, undefined);
  assert.equal(release.on.pull_request,undefined);
  assert.ok(release.on.workflow_call);
  assert.equal(release.jobs['qualified-source']['runs-on'],'ubuntu-24.04');
  assert.equal(release.jobs['qualified-source'].environment,undefined);
  assert.match(release.jobs['qualified-source'].steps[0].with.script,/merge.parents.length !== 2/);
  assert.equal(tests.jobs['release-dry-run'].with.dry_run,true);
  // Packaging overlaps the Rust suites; publication promotes only a fully green run and re-checks its qualification.
  // Packaging starts with the push: it waits for no check, and builds the pushed commit itself.
  assert.deepEqual(tests.jobs['release-dry-run'].needs,['admission','policy']);
  assert.doesNotMatch(tests.jobs['release-dry-run'].if,/needs\.fast/);
  assert.equal(tests.jobs['release-dry-run'].with.source_sha,'${{ github.sha }}');
  assert.match(release.jobs['qualified-source'].steps[0].with.script,/run\.conclusion === 'success'/);
  assert.match(release.jobs.prepare.steps.find(step => step.name === 'Require green dev qualification for the exact release SHA').if,/github\.event_name != 'push'/);
});

test('every privileged reusable release job accepts the qualified caller with protected environment intact', () => {
  for (const name of ['release-linux', 'release-linux-arm64', 'release-windows', 'release-windows-arm64', 'release-macos-build', 'release-manifest-validate', 'release-manifest-dry-run', 'frontend-release']) {
    for (const [id, job] of Object.entries(workflow(name).jobs)) {
      if (job.uses) continue;
      assert.equal(job.environment, 'release', `${name}/${id}`);
      assert.equal(condition(job.if, { github: qualifiedGithub, inputs: { arch: 'x64' } }), true, `${name}/${id}`);
      assert.equal(condition(job.if, { github: { ...qualifiedGithub, actor: 'contributor' }, inputs: { arch: 'x64' } }), false, `${name}/${id}`);
    }
  }
});

test("release pushes qualify even when their merge tree equals a parent", t => {
  const source = fast.jobs.changed.steps.find(step => step.id === "tree");
  const directory = mkdtempSync(join(tmpdir(), "release-tree-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  for (const [branch, expected] of [["release/1.15.3", true], ["lane/opaque", false]]) {
    const full = condition(source.env.CI_RELEASE_CALL, { inputs: { source_ref: "" }, github: { event_name: "push", ref_name: branch } });
    assert.equal(full, expected, branch);
    const output = join(directory, expected ? "release" : "lane");
    const script = "git() { if [ \"$*\" = 'rev-parse HEAD^@' ]; then echo parent; else echo same-tree; fi; }\n" + source.run.replaceAll("${{ github.event_name }}", "push");
    execFileSync(bash, ["-euo", "pipefail", "-c", script], { env: { ...process.env, CI_RELEASE_CALL: String(full), GITHUB_OUTPUT: output } });
    assert.equal(readFileSync(output, "utf8"), `run=${expected}\n`, branch);
  }
});

const fastJobs = ["guards", "plugins", "clippy-linux", "cli-tests", "ts-static", "frontend-windows", "frontend-dist", "ts-types", "ts-unit", "ts-vitest", "ts-plugins"];
const longJobs = ["rust-tests", "rust-platform", "rust-embeddings", "rust-receipts"];
const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";

test("warning collection runs its node wrapper under stock shells and never fails the build", () => {
  const action = parse(readFileSync(new URL("../../.github/actions/rust-warnings/action.yml", import.meta.url), "utf8"));
  assert.deepEqual(action.runs.steps.map(step => step.run), [
    "node scripts/ci-rust-warning-artifact.mjs collect",
    "node scripts/ci-rust-warning-artifact.mjs store",
  ]);
  for (const step of action.runs.steps) {
    assert.equal(step.shell, "${{ runner.os == 'Windows' && 'pwsh' || 'bash' }}");
    assert.equal(step["continue-on-error"], true);
  }
});

test("warning summary stores reports in MinIO without GitHub artifact quota", () => {
  const steps = workflow("rust-warnings-summary").jobs.report.steps;
  assert.ok(steps.every(step => !step.uses?.startsWith("actions/upload-artifact@")));
  const store = steps.find(step => step.name === "Store warning report");
  assert.equal(store.env.WARNING_REPORT_NAME, "${{ inputs.report_name }}");
  assert.ok(store.env.AWS_ACCESS_KEY_ID && store.env.AWS_SECRET_ACCESS_KEY);
  assert.match(store.run, /ci-artifact\.sh put "\$GITHUB_RUN_ID\/\$WARNING_REPORT_NAME\//);
});

const app = { result: "success", outputs: { kind: "app", published: "false", dry_run: "false", run_tests: "true", promote_run_id: "" } };
const frontend = { "frontend-dist": { result: "success" } };
const results = ["success", "failure", "cancelled", "skipped"];

test("normal qualification finishes test siblings and packaging cannot start on failures", () => {
  assert.equal(tests.jobs.rust.with.cancel_run, false);
  assert.equal(tests.jobs.fast.with.cancel_run, false);
  assert.equal(rust.jobs['rust-platform'].strategy['fail-fast'], false);
  const needs = { fast: { result: 'failure', outputs: { resolved_scope: '{}' } } };
  assert.equal(condition(tests.jobs.rust.if, { github: { event_name: 'push' }, needs }), true);
});

test("qualified release branches retain owner admission and a published version never rebuilds", () => {
  assert.equal(release.on.workflow_run, undefined);
  assert.deepEqual(release.on.push,{branches:['master']});
  assert.match(release.jobs.prepare.if, /github.actor == 'rollacode'/);
  const resolve = release.jobs.prepare.steps.find(step => step.id === "release").run;
  assert.match(resolve, /release\/\*\) version="\$\{REQUESTED_REF#release\/\}"/);
  assert.match(resolve, /committed=\$\(node -p "require\('\.\/package\.json'\)\.version"\)/);
  assert.match(resolve, /echo "published=true"/);
  assert.match(resolve, /echo "source_ref=\$sha"/);
});

test("publication never pushes master or develop; administrators own merge PRs", () => {
  assert.equal(release.jobs.promote,undefined);
  assert.doesNotMatch(JSON.stringify(release),/git push origin (master|develop)|gh pr merge/);
});

test("master accepts pull requests only from release branches", () => {
  const script = tests.jobs.policy.steps.find(step => step.name === 'Require a release branch').run;
  for (const [head, ok] of [["release/1.14.4", true], ["release/1.15.0", true], ["develop", false], ["lane/pl-x-task-y", false], ["release/next", false]]) {
    const run = () => execFileSync(bash, ["-euo", "pipefail", "-c", script], { env: { ...process.env, SOURCE_REPO: "rollacode/codeterm", HEAD_REF: head, HEAD_REPO: "rollacode/codeterm", PR_AUTHOR: "rollacode" }, stdio: "pipe" });
    if (ok) run(); else assert.throws(run, undefined, head);
  }
});

test("platform builders do not push release tags before the test gate", () => {
  for (const name of ["release-macos-build", "release-linux-arm64", "release-linux", "release-windows"]) {
    for (const job of Object.values(workflow(name).jobs)) {
      for (const step of job.steps ?? []) assert.doesNotMatch(step.run || "", /git push/);
    }
  }
  const publisher = workflow("release-manifest-validate");
  const stamp = publisher.jobs.validate.steps.find(step => step.name === "Stamp the source release tag after all gates succeed");
  assert.ok(stamp);
  assert.match(stamp.run, /git push --force origin/);
  assert.match(stamp.run, /test .* = "\$SOURCE_SHA"/);
});

test("heavy Rust and build jobs end by removing their own targets and archives", () => {
  const heavy = [
    ...["rust-tests", "rust-platform", "rust-embeddings"].map(id => rust.jobs[id]),
    ...["clippy-linux"].map(id => fast.jobs[id]),
    ...["release-macos-build", "release-linux", "release-linux-arm64", "release-windows"].map(name => workflow(name).jobs.build),
  ];
  for (const job of heavy) {
    const last = job.steps.at(-1);
    assert.equal(last.run, "node scripts/ci-workspace-cleanup.mjs", job.name);
    assert.equal(last.if, "always()");
    assert.equal(last["continue-on-error"], true, "cleanup never changes the job result");
  }
});

test("CI jobs queue for a shared builder's gateway slot instead of failing busy", () => {
  for (const name of ["tests-fast", "tests-rust", "release-windows", "release-macos-build", "release-linux"]) {
    assert.equal(workflow(name).env.CODETERM_BUILD_GATEWAY_MODE, "queue", name);
  }
});


test("guards preflight prerequisites before cargo-dependent serial contracts", () => {
  const steps = fast.jobs.guards.steps;
  const setup = steps.findIndex(step => step.run?.includes('ci-runner-paths.mjs'));
  const serial = steps.findIndex(step => step.name === 'Build gateway guards and serial gateway script tests');
  assert.ok(setup >= 0 && setup < serial);
});

test("only the release push qualifies a release; develop pushes and release PRs start no CI run", () => {
  assert.deepEqual(tests.on.push.branches, ['master', 'rename/domios', 'release/**']);
  assert.deepEqual(tests.on.pull_request['branches-ignore'], ['master']);
  assert.equal(tests.on.push['paths-ignore'], undefined);
  assert.equal(tests.on.schedule.length, 2);
  assert.match(tests.concurrency.group, /github.ref/);
  assert.equal(tests.concurrency['cancel-in-progress'], true);
  for (const event_name of ['push', 'schedule']) {
    assert.equal(condition(tests.jobs.rust.if, { github: { event_name }, needs: {
      fast: { outputs: { resolved_scope: '{}' } } } }), true);
  }
  assert.equal(tests.jobs.rust.with.resolved_scope, '${{ needs.fast.outputs.resolved_scope }}');
  assert.equal(tests.jobs.rust.with.source_ref, '${{ needs.fast.outputs.source_sha }}');
  assert.equal(rust.jobs.changed, undefined, 'the Rust tier reuses the caller scope without a second job');
  assert.equal(rust.on.workflow_call.inputs.resolved_scope.required, true);
  const source = fast.jobs.changed.steps.find(step => step.id === 'tree');
  assert.match(source.env.CI_RELEASE_CALL, /develop/);
  assert.match(source.env.CI_RELEASE_CALL, /rename\/domios/);
  const qualify = tests.jobs.qualification;
  assert.deepEqual(qualify.needs, ['admission', 'fast', 'rust', 'rust-warnings', 'policy']);
  assert.match(qualify.if, /needs.fast.outputs.heavy == 'true'/);
  assert.equal(qualify.steps.at(-1).with.name, 'qualification-${{ needs.fast.outputs.source_sha }}');
  const prepare = release.jobs.prepare.steps;
  const check = prepare.findIndex(step => /requireQualificationWithGithub\(github, context, sha\)/.test(step.with?.script ?? ''));
  const drafts = prepare.findIndex(step => step.name === 'Create draft releases before parallel uploads');
  assert.ok(check >= 0 && check < drafts);
  assert.equal(prepare[check].if, "${{ !inputs.dry_run || github.event_name != 'push' }}", 'Release push packaging follows its own successful qualification dependency');
  for (const id of ['tests-tier', 'builds-tier', 'quick-checks', 'tests']) assert.equal(release.jobs[id], undefined);
  for (const id of builds) assert.deepEqual(release.jobs[id].needs, ['prepare', 'frontend-dist']);
  for (const file of ['release-promote.mjs', 'release-publish.sh']) {
    const source = readFileSync(new URL('../' + file, import.meta.url), 'utf8');
    assert.match(source, /requireQualification|release-promote.mjs check/);
  }
  assert.ok(workflow('release-manifest-validate').jobs.validate.steps.some(step => /requireQualificationWithGithub\(github, context, sha\)/.test(step.with?.script ?? '')));
});

test("Rust qualification stays self-hosted and hosted frontend jobs have no secrets or private cache", () => {
  for (const job of Object.values(rust.jobs)) {
    assert.ok(job['runs-on'].includes('self-hosted'));
  }
  for (const id of ['clippy-linux', 'cli-tests']) assert.ok(fast.jobs[id]['runs-on'].includes('self-hosted'));
  for (const id of ['ts-unit', 'ts-vitest', 'ts-static', 'ts-types', 'ts-plugins', 'frontend-windows', 'frontend-dist', 'plugins']) {
    const job = fast.jobs[id];
    assert.match(job['runs-on'], /^(ubuntu-24.04|windows-latest)$/);
    assert.doesNotMatch(JSON.stringify(job), /secrets\.|ci-artifact\.sh|ci-artifact-spool/);
    assert.equal(job.environment, undefined);
    assert.equal(job.env.CODETERM_USE_SCCACHE, '0');
    assert.ok(Object.values(job.permissions).every(value => value !== 'write'));
  }
});

test("the frontend producer owns tsc and Windows keeps the focused TS2451 path oracle", () => {
  const commands = fast.jobs['ts-static'].steps.map(step => step.run ?? '').join('\n');
  assert.doesNotMatch(commands, /ts-typecheck|tsc/);
  for (const entry of ['ts-unused-exports', 'ts-knip-report', 'ts-api-effects', 'ts-biome']) assert.ok(commands.includes(entry));
  assert.ok(fast.jobs['frontend-dist'].steps.some(step => /(ci-catalogue-command|ci-step).mjs ts-typecheck/.test(step.run ?? '')));
  const windows = fast.jobs['frontend-windows'];
  assert.ok(windows.steps.some(step => /(ci-catalogue-command|ci-step).mjs frontend-windows-build/.test(step.run ?? '')));
  assert.ok(windows.steps.every(step => !/frontend-build|pnpm build/.test(step.run ?? '')));
  const fixture = readFileSync(new URL('./windows-path-identity.test.mjs', import.meta.url), 'utf8');
  assert.match(fixture, /subst/);
  assert.match(fixture, /2451/);
  assert.match(fixture, /preserveSymlinks: true/);
});

test("every publication stage fails closed on failed preparation and package builds", () => {
  const prepare = { result: 'success', outputs: { kind: 'app', published: 'false', dry_run: 'false', promote_run_id: '' } };
  const ok = Object.fromEntries(builds.map(id => [id, { result: 'success' }]));
  const needs = { ...ok, prepare, 'promote-evidence': { result: 'skipped' } };
  const vars = { RELEASE_PUBLISH_ENABLED: 'true' };
  assert.equal(condition(release.jobs['release-manifest'].if, { needs, vars }), true);
  for (const id of builds) for (const result of ['failure', 'skipped', 'cancelled']) {
    assert.equal(condition(release.jobs['release-manifest'].if, { needs: { ...needs, [id]: { result } }, vars }), false);
  }
  for (const id of builds) {
    assert.equal(condition(release.jobs[id].if, { needs: { prepare: { ...prepare, result: 'failure' }, 'frontend-dist': { result: 'success' } } }), false);
  }
  assert.equal(condition(release.jobs['release-manifest'].if, { needs, vars: {} }), false);
});

test("shared fast failure hooks use job-local source identity without foreign step references", () => {
  const source = fast.jobs.changed.steps.find(step => step.id === "source");
  assert.match(source.run, /sha=\$\(git rev-parse HEAD\)/);
  assert.match(source.run, /CI_SOURCE_SHA=\$sha.*GITHUB_ENV/);
  for (const [id, job] of Object.entries(fast.jobs)) {
    if (id !== "changed") assert.equal(job.env.CI_SOURCE_SHA, "${{ needs.changed.outputs.source_sha || github.sha }}", id);
    for (const hook of job.steps.filter(step => /ci-first-failure/.test(step.run ?? ""))) {
      assert.equal(hook.env.CI_SOURCE_SHA, "${{ env.CI_SOURCE_SHA }}", id);
    }
  }
});
