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

const fastJobs = ["guards", "plugins", "clippy-linux", "cli-tests", "ts-static", "frontend-windows", "ts-types", "ts-unit", "ts-vitest", "ts-plugins"];
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

test("one fast-check caller runs, and it cancels the run when a check fails", () => {
  const job = release.jobs["quick-checks"];
  assert.equal(job.uses, "./.github/workflows/tests-fast.yml");
  assert.equal(job.with.phase, undefined);
  assert.deepEqual(dependencies(job), ["prepare"]);
  assert.equal(job.with.source_ref, "${{ needs.prepare.outputs.source_sha }}");
  assert.equal(job.with.cancel_run, true);
  const callers = Object.entries(release.jobs).filter(([, entry]) => entry.uses?.startsWith("./.github/workflows/tests")).map(([id, entry]) => [id, entry.uses]);
  assert.deepEqual(callers.sort(), [["quick-checks", "./.github/workflows/tests-fast.yml"], ["tests", "./.github/workflows/tests-rust.yml"]]);
  for (const id of [...builds, "tests", "quick-checks"]) assert.notEqual(release.jobs[id]["continue-on-error"], true);
});

test("tests open once every fast check is claimed; builds open once every Rust test job is claimed", () => {
  const gate = release.jobs["tests-tier"];
  assert.deepEqual(dependencies(gate), ["prepare"]);
  assert.equal(gate.env.TIER_PREFIX, "Fast checks / ");
  assert.equal(gate.env.TIER_ANCHOR, "Fast checks / guards");
  assert.ok(gate.steps.some(step => step.run === 'node scripts/release-tier-gate.mjs "$TIER_PREFIX" "$TIER_ANCHOR" 45'));
  assert.ok(gate["timeout-minutes"] > 45, "the gate outlives its own deadline");
  const buildsGate = release.jobs["builds-tier"];
  assert.deepEqual(dependencies(buildsGate), ["prepare", "tests-tier"]);
  assert.equal(buildsGate.env.TIER_PREFIX, "Rust tests / ");
  assert.equal(buildsGate.env.TIER_ANCHOR, "Rust tests / tree is new");
  assert.ok(buildsGate.steps.some(step => step.run === 'node scripts/release-tier-gate.mjs "$TIER_PREFIX" "$TIER_ANCHOR" 45'));
  assert.ok(buildsGate["timeout-minutes"] > 45, "the gate outlives its own deadline");
  for (const result of results) {
    assert.equal(condition(buildsGate.if, { needs: { prepare: app, "tests-tier": { result } } }), result === "success", result);
  }
  assert.deepEqual(dependencies(release.jobs.tests), ["prepare", "tests-tier"]);
  for (const result of results) {
    assert.equal(condition(release.jobs.tests.if, { needs: { prepare: app, "tests-tier": { result } } }), result === "success", result);
  }
  assert.deepEqual(dependencies(release.jobs["frontend-dist"]), ["prepare"]);
  assert.equal(condition(release.jobs["frontend-dist"].if, { needs: { prepare: app } }), true);
  for (const id of builds) {
    const job = release.jobs[id];
    assert.deepEqual(dependencies(job), ["prepare", "frontend-dist", "builds-tier"], id);
    for (const result of results) {
      assert.equal(condition(job.if, { needs: { prepare: app, ...frontend, "builds-tier": { result } } }), result === "success", `${id} ${result}`);
    }
    assert.equal(condition(job.if, { needs: { prepare: app, ...frontend, "builds-tier": { result: "success" } }, cancelled: true }), false);
    assert.equal(condition(job.if, { needs: { prepare: app, "frontend-dist": { result: "failure" } } }), false, id);
  }
  assert.equal(release.jobs.tests.with.phase, undefined);
  assert.deepEqual(new Set(dependencies(release.jobs["release-manifest"])), new Set(["prepare", "quick-checks", "tests", "promote-evidence", ...builds]));
});

test("dry runs use both test stages by default; bundle-only diagnostics explicitly bypass them", () => {
  assert.equal(release.on.workflow_dispatch.inputs.dry_run_tests.default, true);
  const base = "needs.prepare.outputs.run_tests == 'true' && needs.prepare.outputs.published != 'true'";
  assert.equal(release.jobs["quick-checks"].if, base);
  const dry = { result: "success", outputs: { kind: "app", published: "false", dry_run: "true", run_tests: "false", promote_run_id: "" } };
  for (const id of builds) {
    assert.equal(release.jobs[id].with.dry_run, "${{ needs.prepare.outputs.dry_run == 'true' }}");
    const needs = { prepare: dry, ...frontend, "quick-checks": { result: "skipped" }, "builds-tier": { result: "skipped" } };
    assert.equal(condition(release.jobs[id].if, { needs, inputs: { dry_run_tests: false } }), true, id);
  }
  const draft = release.jobs.prepare.steps.find(step => step.name === "Create draft releases before parallel uploads");
  assert.equal(draft.if, "steps.release.outputs.dry_run != 'true' && steps.release.outputs.published != 'true' && steps.release.outputs.kind == 'app'");
});

test("guards gate every other fast check, and the two tier workflows partition every check", () => {
  assert.deepEqual(Object.keys(fast.jobs).sort(), ["changed", ...fastJobs].sort());
  assert.deepEqual(Object.keys(rust.jobs).sort(), ["changed", ...longJobs].sort());
  assert.deepEqual(fast.jobs.changed, rust.jobs.changed, "both tiers gate on one scope decision");
  const needs = { changed: { result: "success", outputs: { heavy: "true", rust: "true", frontend: "true", plugins: '[{"id":"test"}]' } }, guards: { result: "success" }, "rust-tests": { result: "success" }, "rust-platform": { result: "success" }, "rust-embeddings": { result: "success" } };
  for (const id of fastJobs) {
    assert.deepEqual(dependencies(fast.jobs[id]), ["changed"], id);
    assert.equal(fast.jobs[id].if === undefined || condition(fast.jobs[id].if, { needs }), true, id);
  }
  const steps = fast.jobs.guards.steps.filter(step => step.run && !/gh run cancel/.test(step.run));
  assert.equal(steps[0].id, "catalogue");
  assert.equal(steps[1].run, "bash scripts/check-file-sizes.sh --fail", "file sizes run first");
  for (const step of steps.slice(2)) assert.match(step.if, /^\$\{\{ !cancelled\(\)/, `${step.name}: one red guard still reports the rest`);
  const skipped = { ...needs, guards: { result: "skipped" } };
  for (const id of ["rust-tests", "rust-platform", "rust-embeddings"]) {
    assert.deepEqual(dependencies(rust.jobs[id]), ["changed"], id);
    assert.ok(condition(rust.jobs[id].if, { needs: skipped }), `${id}: the caller orders the tiers`);
    assert.equal(condition(rust.jobs[id].if, { needs, cancelled: true }), false, `${id} cancelled`);
  }
  assert.equal(condition(rust.jobs["rust-receipts"].if, { needs }), true);
  for (const tier of [fast, rust]) {
    assert.equal(tier.concurrency, undefined, "a tier runs inside its caller's concurrency group");
    assert.deepEqual(Object.keys(tier.on), ["workflow_call"]);
  }
  assert.deepEqual(Object.keys(tests.jobs), ["fast", "rust", "rust-warnings"]);
  assert.equal(tests.jobs.fast.uses, "./.github/workflows/tests-fast.yml");
  assert.equal(tests.jobs.rust.uses, "./.github/workflows/tests-rust.yml");
  assert.deepEqual(dependencies(tests.jobs.rust), ["fast"]);
  for (const event of ["push", "pull_request", "workflow_dispatch"]) {
    assert.equal(condition(tests.jobs.rust.if, { github: { event_name: event } }), event === "push", event);
  }
});

test("both test tiers cancel the current release run before cleanup", () => {
  for (const tier of [fast, rust]) {
    assert.equal(tier.on.workflow_call.inputs.cancel_run.default, false);
    assert.equal(tier.env.CI_CANCEL_RUN, "${{ inputs.cancel_run }}");
    assert.equal(tier.env.DOMIOS_CI_WEBHOOK_URL, "${{ secrets.DOMIOS_CI_WEBHOOK_URL }}");
    assert.equal(tier.env.DOMIOS_CI_WEBHOOK_TOKEN, "${{ secrets.DOMIOS_CI_WEBHOOK_TOKEN }}");
    for (const [id, job] of Object.entries(tier.jobs)) {
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
  assert.equal(release.jobs["quick-checks"].with.cancel_run, true);
  assert.equal(release.jobs.tests.with.cancel_run, true);
  assert.equal(tests.jobs.fast.with.cancel_run, undefined);
  assert.equal(tests.jobs.rust.with.cancel_run, undefined);
});

test("only a dry run tolerates refused artifact uploads, and every Rust test job stays required", () => {
  for (const tier of [fast, rust]) assert.equal(tier.on.workflow_call.inputs.artifacts_optional.default, false);
  for (const id of ["quick-checks", "tests"]) assert.equal(release.jobs[id].with.artifacts_optional, "${{ needs.prepare.outputs.dry_run == 'true' }}", id);
  const tolerant = Object.entries({ ...fast.jobs, ...rust.jobs }).flatMap(([id, job]) => (job.steps || [])
    .filter(step => step["continue-on-error"] === "${{ inputs.artifacts_optional }}").map(step => [id, step]));
  for (const [id, step] of tolerant) {
    assert.ok(/bash scripts\/ci-artifact\.sh (put|get-prefix) /.test(step.run ?? "") || step.run === "node scripts/ci-rust-tests.mjs verify ci-rust-receipts", `${id}: only artifact transport and receipt verification may be optional`);
  }
  assert.deepEqual(tolerant.map(([id]) => id).sort(), ["frontend-windows", "rust-embeddings", "rust-platform", "rust-receipts", "rust-receipts", "rust-tests"]);
  const required = rust.jobs["rust-receipts"].steps.find(step => step.name === "Require every Rust test job");
  assert.equal(required["continue-on-error"], undefined);
});

test("publication needs every stage to succeed", () => {
  const vars = { RELEASE_PUBLISH_ENABLED: "true" };
  const ok = Object.fromEntries([...builds, "tests"].map(id => [id, { result: "success" }]));
  const manifest = release.jobs["release-manifest"];
  const needs = { ...ok, prepare: app, "quick-checks": { result: "success" }, "promote-evidence": { result: "skipped" } };
  assert.equal(condition(manifest.if, { needs, vars }), true);
  assert.equal(condition(manifest.if, { needs, vars: {} }), false, "publication stays off without the variable");
  for (const id of [...builds, "tests", "quick-checks"]) {
    for (const result of ["failure", "cancelled", "skipped"]) {
      assert.equal(condition(manifest.if, { needs: { ...needs, [id]: { result } }, vars }), false, `${id}: ${result}`);
    }
  }
  for (const id of ["release-manifest", "frontend"]) {
    assert.ok(["tests", "quick-checks"].every(need => dependencies(release.jobs[id]).includes(need)), id);
  }
});

test("a push to release/X.Y.Z by the owner account is the release, and a published version never rebuilds", () => {
  assert.deepEqual(release.on.push, { branches: ["release/**"] });
  assert.equal(release.on.push.tags, undefined);
  assert.equal(release.jobs.prepare.if, "github.event_name != 'push' || github.actor == 'rollacode'");
  const resolve = release.jobs.prepare.steps.find(step => step.id === "release").run;
  assert.match(resolve, /release\/\*\) version="\$\{REQUESTED_REF#release\/\}"/);
  assert.match(resolve, /committed=\$\(node -p "require\('\.\/package\.json'\)\.version"\)/);
  assert.match(resolve, /echo "published=true"/);
  assert.match(resolve, /echo "source_ref=\$sha"/);
});

test("only a successful publication merges the release into master and back into develop", () => {
  const promote = release.jobs.promote;
  assert.deepEqual(new Set(dependencies(promote)), new Set(["prepare", "release-manifest", "frontend"]));
  assert.match(promote.if, /^always\(\) && needs\.prepare\.outputs\.branch != '' && needs\.prepare\.outputs\.published != 'true'/);
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
    const run = () => execFileSync(bash, ["-euo", "pipefail", "-c", script], { env: { ...process.env, HEAD_REF: head }, stdio: "pipe" });
    if (ok) run(); else assert.throws(run, undefined, head);
  }
});

test("release calls pass the release SHA and force scope through both skip gates", () => {
  assert.ok(Object.hasOwn(tests.on, "pull_request"));
  assert.ok(Object.hasOwn(tests.on, "workflow_dispatch"));
  assert.equal(tests.on.workflow_call, undefined, "release.yml calls the tiers directly");
  assert.equal(tests.on.schedule, undefined, "no nightly trigger without a scheduled catalogue tier");
  assert.deepEqual(Object.keys(fast.on.workflow_call.inputs), ["source_ref", "scope", "cancel_run", "artifacts_optional"]);
  assert.deepEqual(Object.keys(rust.on.workflow_call.inputs), ["source_ref", "scope", "cancel_run", "artifacts_optional"]);
  for (const id of ["quick-checks", "tests"]) assert.equal(release.jobs[id].with.source_ref, "${{ needs.prepare.outputs.source_sha }}", id);
  for (const tier of [fast, rust]) {
    assert.equal(tier.on.workflow_call.inputs.source_ref.type, "string");
    const changed = tier.jobs.changed;
    assert.equal(changed.if, "${{ github.event_name != 'pull_request' || !startsWith(github.head_ref, 'release/') }}", "only a release branch's own pull request skips; release calls inherit the push event");
    assert.equal(changed.steps.find(step => step.id === "tree").env.CI_RELEASE_CALL, "${{ inputs.source_ref != '' }}");
    const scopeEnv = changed.steps.find(step => step.id === "scope").env;
    assert.equal(scopeEnv.CI_FULL_MATRIX, "${{ (inputs.source_ref != '' && inputs.scope != 'frontend') }}");
    assert.equal(scopeEnv.CI_FRONTEND_ONLY, "${{ inputs.source_ref != '' && inputs.scope == 'frontend' }}");
    for (const job of Object.values(tier.jobs)) {
      for (const step of (job.steps || []).filter(step => step.uses?.startsWith("actions/checkout@"))) {
        assert.equal(step.with.ref, "${{ inputs.source_ref || github.ref }}");
      }
    }
  }
});

test("release pushes cannot skip an unchanged tree", () => {
  const script = fast.jobs.changed.steps.find(step => step.id === "tree").run
    .replaceAll("${{ github.event_name }}", "push");
  // A CI runner's stdout can be a socket, where /dev/stdout is not appendable.
  const output = join(mkdtempSync(join(tmpdir(), "release-gate-")), "output");
  execFileSync(bash, ["-euo", "pipefail", "-c", script], {
    encoding: "utf8",
    env: { ...process.env, CI_RELEASE_CALL: "true", GITHUB_OUTPUT: output },
  });
  assert.equal(readFileSync(output, "utf8").trim(), "run=true");
});

test("platform builders do not push release tags before the test gate", () => {
  for (const name of ["release-macos-build", "release-linux-arm64", "release-linux", "release-windows"]) {
    for (const job of Object.values(workflow(name).jobs)) {
      for (const step of job.steps) assert.doesNotMatch(step.run || "", /git push/);
    }
  }
  const publisher = workflow("release-manifest-validate");
  const stamp = publisher.jobs.validate.steps.find(step => step.name === "Stamp the source release tag after all gates succeed");
  assert.ok(stamp);
  assert.match(stamp.run, /git push --force origin/);
  assert.match(stamp.run, /test .* = "\$SOURCE_SHA"/);
});

test("a frontend-only release builds and publishes only the frontend after the frontend tests", () => {
  const frontend = release.jobs.frontend;
  assert.equal(frontend.uses, "./.github/workflows/frontend-release.yml");
  assert.deepEqual(new Set(dependencies(frontend)), new Set(["prepare", "quick-checks", "tests"]));
  const frontendRelease = { result: "success", outputs: { kind: "frontend", published: "false", dry_run: "false" } };
  const needs = { prepare: frontendRelease, tests: { result: "success" }, "quick-checks": { result: "success" } };
  const vars = { RELEASE_PUBLISH_ENABLED: "true" };
  assert.equal(condition(frontend.if, { needs, vars }), true);
  assert.equal(condition(frontend.if, { needs: { ...needs, tests: { result: "failure" } }, vars }), false);
  assert.equal(condition(frontend.if, { needs, vars: {} }), false);
  assert.deepEqual(frontend.with, { tag: "${{ needs.prepare.outputs.tag }}", source_sha: "${{ needs.prepare.outputs.source_sha }}" });
  const resolve = release.jobs.prepare.steps.find(step => step.id === "release").run;
  assert.match(resolve, /kind=\$\(node scripts\/ci-scope\.mjs --release-kind "\$previous" "\$sha"\)/);
  assert.match(resolve, /tag="frontend-v\$version"/);
  const called = workflow("frontend-release");
  assert.deepEqual(Object.keys(called.on.workflow_call.inputs), ["tag", "source_sha"]);
  assert.equal(called.on.push, undefined, "a frontend tag push no longer publishes outside the release run");
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

test("the full suite runs once on the rust-tests pool and each OS runs only its platform subset in place", () => {
  const full = rust.jobs["rust-tests"];
  assert.deepEqual(full["runs-on"], ["self-hosted", "rust-tests"]);
  assert.deepEqual(full.strategy.matrix.suite, ["full"]);
  const platform = rust.jobs["rust-platform"];
  assert.ok(platform.strategy.matrix.include.every(entry => entry.suite === "platform"));
  assert.deepEqual(platform.strategy.matrix.include.map(entry => entry.os).sort(), ["Linux", "Windows", "macOS"]);
  assert.equal(platform.strategy["fail-fast"], false);
  const embeddings = rust.jobs["rust-embeddings"];
  assert.deepEqual(embeddings.strategy.matrix.suite, ["embeddings"]);
  assert.deepEqual(embeddings["runs-on"], ["self-hosted", "rust-tests"]);
  for (const job of [full, platform, embeddings]) {
    const runs = job.steps.map(step => step.run ?? "").join(" ; ");
    assert.match(runs, /build\.sh ci-tests \$\{\{ matrix\.suite \}\}/);
    assert.match(runs, /build\.ps1 ci-tests \$\{\{ matrix\.suite \}\}/);
    assert.ok(!job.steps.some(step => step.uses?.startsWith("actions/download-artifact")), "no archive download");
  }
  assert.deepEqual(new Set(dependencies(rust.jobs["rust-receipts"])), new Set(["changed", "rust-tests", "rust-platform", "rust-embeddings"]));
});

test("platform tests expand to one job per OS", () => {
  const { include, ...axes } = rust.jobs["rust-platform"].strategy.matrix;
  assert.deepEqual(Object.keys(axes), [], "added include keys on a base combination overwrite each other");
  assert.deepEqual(include.map(row => [row.suite, row.os]), [["platform", "Linux"], ["platform", "macOS"], ["platform", "Windows"]]);
  for (const row of include) assert.ok(row.label, row.os);
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
