import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
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

test("both qualification tiers notify and cancel immediately before cleanup", () => {
  for (const tier of [fast, rust]) {
    assert.equal(tier.on.workflow_call.inputs.cancel_run.default, false);
    assert.equal(tier.env.CI_CANCEL_RUN, "${{ inputs.cancel_run }}");
    for (const [id, job] of Object.entries(tier.jobs)) {
      assert.equal(job.env.DOMIOS_CI_WEBHOOK_URL, "${{ secrets.DOMIOS_CI_WEBHOOK_URL }}");
      assert.equal(job.env.DOMIOS_CI_WEBHOOK_TOKEN, "${{ secrets.DOMIOS_CI_WEBHOOK_TOKEN }}");
      const hooks = job.steps.filter(step => /ci-first-failure/.test(step.run ?? ""));
      assert.ok(hooks.length > 0, id);
      assert.equal(job.permissions.actions, "write", id);
      for (const hook of hooks) {
        assert.equal(hook.if, "failure() && inputs.cancel_run", id);
        assert.equal(hook["continue-on-error"], true, id);
        assert.match(hook.run, /gh run cancel "\$GITHUB_RUN_ID" --repo "\$GITHUB_REPOSITORY"/);
        assert.ok(hook.run.indexOf("ci-first-failure") < hook.run.indexOf("gh run cancel"));
        assert.equal(hook.env.CI_FAILURE_STEPS, "${{ toJSON(steps) }}");
      }
      for (let index = 0; index < job.steps.length; index++) {
        const step = job.steps[index];
        if (hooks.includes(step) || step.if?.startsWith("always()")) continue;
        assert.ok(hooks.includes(job.steps[index + 1]), `${id}/${step.name || step.id}: immediate hook`);
      }
    }
  }
  assert.equal(tests.jobs.fast.with.cancel_run, "${{ github.event_name != 'pull_request' }}");
  assert.equal(tests.jobs.rust.with.cancel_run, true);
});

test("a push to release/X.Y.Z by the owner account is the release, and a published version never rebuilds", () => {
  assert.deepEqual(release.on.push, { branches: ["release/**"] });
  assert.equal(release.on.push.tags, undefined);
  assert.match(release.jobs.prepare.if, /github.actor == 'rollacode'/);
  const resolve = release.jobs.prepare.steps.find(step => step.id === "release").run;
  assert.match(resolve, /release\/\*\) version="\$\{REQUESTED_REF#release\/\}"/);
  assert.match(resolve, /committed=\$\(node -p "require\('\.\/package\.json'\)\.version"\)/);
  assert.match(resolve, /echo "published=true"/);
  assert.match(resolve, /echo "source_ref=\$sha"/);
});

test("only a successful publication merges the release into master and back into develop", () => {
  const promote = release.jobs.promote;
  assert.deepEqual(new Set(dependencies(promote)), new Set(["prepare", "release-manifest", "frontend"]));
  assert.match(promote.if, /always\(\) && needs\.prepare\.outputs\.branch != '' && needs\.prepare\.outputs\.published != 'true'/);
  assert.match(promote.if, /needs\.release-manifest\.result == 'success' \|\| needs\.frontend\.result == 'success'/);
  const scripts = promote.steps.map(step => step.run ?? "").join("\n");
  assert.match(scripts, /gh pr merge "\$pr" --repo "\$REPO" --merge/);
  assert.match(scripts, /git merge-base --is-ancestor "\$SOURCE_SHA" origin\/master/);
  assert.match(scripts, /git push origin develop/);
});

test("master accepts pull requests only from release branches", () => {
  const guard = workflow("master-pr-source");
  assert.deepEqual(guard.on.pull_request.branches, ["master"]);
  const script = guard.jobs.source.steps[0].run;
  for (const [head, ok] of [["release/1.14.4", true], ["release/1.15.0", true], ["develop", false], ["lane/pl-x-task-y", false], ["release/next", false]]) {
    const run = () => execFileSync(bash, ["-euo", "pipefail", "-c", script], { env: { ...process.env, HEAD_REF: head, HEAD_REPO: "rollacode/codeterm", PR_AUTHOR: "rollacode" }, stdio: "pipe" });
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
    ...["clippy-linux", "ts-types"].map(id => fast.jobs[id]),
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


test("guards prepare Rust before cargo-dependent serial script tests in every scope", () => {
  const steps = fast.jobs.guards.steps;
  const setup = steps.findIndex(step => step.name === "Ensure Rust toolchain on PATH");
  const serial = steps.findIndex(step => step.name === "Build gateway guards and serial gateway script tests");
  assert.ok(setup >= 0 && setup < serial);
  assert.equal(steps[setup].if, "${{ !cancelled() }}");
});

test("dev pushes and nightly fallback qualify every exact SHA, and release reuses the evidence", () => {
  assert.deepEqual(tests.on.push.branches, ['master', 'develop', 'rename/domios', 'release/**']);
  assert.equal(tests.on.push['paths-ignore'], undefined);
  assert.equal(tests.on.schedule.length, 2);
  assert.match(tests.concurrency.group, /github.ref/);
  assert.equal(tests.concurrency['cancel-in-progress'], true);
  assert.equal(tests.jobs.rust.if, "github.event_name == 'push' || github.event_name == 'schedule'");
  assert.equal(tests.jobs.rust.with.resolved_scope, '${{ needs.fast.outputs.resolved_scope }}');
  assert.equal(tests.jobs.rust.with.source_ref, '${{ needs.fast.outputs.source_sha }}');
  assert.equal(rust.jobs.changed, undefined, 'the Rust tier reuses the caller scope without a second job');
  assert.equal(rust.on.workflow_call.inputs.resolved_scope.required, true);
  const source = fast.jobs.changed.steps.find(step => step.id === 'tree');
  assert.match(source.env.CI_RELEASE_CALL, /develop/);
  assert.match(source.env.CI_RELEASE_CALL, /rename\/domios/);
  const qualify = tests.jobs.qualification;
  assert.deepEqual(qualify.needs, ['fast', 'rust', 'rust-warnings']);
  assert.match(qualify.if, /needs.fast.outputs.heavy == 'true'/);
  assert.equal(qualify.steps.at(-1).with.name, 'qualification-${{ needs.fast.outputs.source_sha }}');
  const prepare = release.jobs.prepare.steps;
  const check = prepare.findIndex(step => /ci-qualification.mjs check/.test(step.run ?? ''));
  const drafts = prepare.findIndex(step => step.name === 'Create draft releases before parallel uploads');
  assert.ok(check >= 0 && check < drafts);
  assert.equal(prepare[check].if, undefined, 'dry runs cannot bypass qualification');
  for (const id of ['tests-tier', 'builds-tier', 'quick-checks', 'tests']) assert.equal(release.jobs[id], undefined);
  for (const id of builds) assert.deepEqual(release.jobs[id].needs, ['prepare', 'frontend-dist']);
  for (const file of ['release-promote.mjs', 'release-publish.sh']) {
    const source = readFileSync(new URL('../' + file, import.meta.url), 'utf8');
    assert.match(source, /requireQualification|release-promote.mjs check/);
  }
  assert.ok(workflow('release-manifest-validate').jobs.validate.steps.some(step => /ci-qualification.mjs check/.test(step.run ?? '')));
});

test("stable physical-host compile owners never share mutable targets or occupy light runners", () => {
  assert.deepEqual(rust.jobs['rust-tests']['runs-on'], ['self-hosted', 'Linux', 'rust-tests', 'heavy', 'host-blackstar']);
  const rows = rust.jobs['rust-platform'].strategy.matrix.include;
  assert.deepEqual(rows.map(row => row.os), ['macOS', 'Windows']);
  assert.deepEqual(rows.map(row => row.label), ['host-air', 'host-rog']);
  assert.equal(rust.jobs['rust-platform'].needs, undefined, 'native platforms use independent physical hosts');
  assert.deepEqual(rust.jobs['rust-embeddings']['runs-on'], ['self-hosted', 'macOS', 'rust-shard', 'host-air']);
  assert.ok(rust.jobs['rust-embeddings'].needs.includes('rust-platform'), 'embeddings reuse Air after native tests');
  for (const [id, job] of Object.entries(fast.jobs)) {
    const idleRustHost = (id === 'ts-unit' && job['runs-on'].includes('host-air')) || (id === 'guards' && job['runs-on'].includes('host-blackstar'));
    assert.ok(job['runs-on'].includes('host-ubunta') || id === 'frontend-windows' || idleRustHost, id);
    assert.ok(!job['runs-on'].includes('rust-tests'), id);
  }
  for (const id of ['clippy-linux', 'cli-tests', 'ts-types']) {
    assert.deepEqual(fast.jobs[id]['runs-on'], ['self-hosted', 'Linux', 'linux-build', 'host-ubunta']);
  }
  for (const id of ['rust-tests', 'rust-platform', 'rust-embeddings']) {
    assert.match(rust.jobs[id].env.CI_TARGET_ROOT, /github.workspace/);
  }
});

test("the frontend producer owns tsc and Windows keeps the focused TS2451 path oracle", () => {
  const commands = fast.jobs['ts-static'].steps.map(step => step.run ?? '').join('\n');
  assert.doesNotMatch(commands, /ts-typecheck|tsc/);
  for (const entry of ['ts-unused-exports', 'ts-knip-report', 'ts-api-effects', 'ts-biome']) assert.ok(commands.includes(entry));
  assert.ok(fast.jobs['frontend-dist'].steps.some(step => step.run?.includes("['ts-typecheck']")));
  const windows = fast.jobs['frontend-windows'];
  assert.ok(windows.steps.some(step => step.run?.includes("['frontend-windows-build']")));
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
