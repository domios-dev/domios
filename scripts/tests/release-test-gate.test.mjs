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
const builds = ["macos-arm64", "macos-x64", "linux-arm64", "linux-x86_64", "windows-x64", "windows-arm64"];
const dependencies = job => [job.needs].flat();

const fastJobs = ["file-sizes", "plugin-metadata", "plugins", "fmt", "clippy-linux", "ts-static", "frontend-windows", "ts-types", "ts-unit", "ts-vitest", "ts-plugins", "release-contracts"];
const longJobs = ["rust-tests", "rust-platform", "rust-receipts"];

const overlapped = ["macos-arm64", "macos-x64", "linux-arm64", "windows-x64", "windows-arm64"];
const afterFast = ["linux-x86_64", "tests"];
const app = { result: "success", outputs: { kind: "app", published: "false", dry_run: "false" } };
const results = ["success", "failure", "cancelled", "skipped"];

test("one fast-check caller runs per mode, and the overlapping one cancels the run when it fails", () => {
  for (const id of ["quick-checks", "fast-gate"]) {
    const job = release.jobs[id];
    assert.equal(job.uses, "./.github/workflows/tests.yml");
    assert.equal(job.with.phase, "fast");
    assert.deepEqual(dependencies(job), ["prepare"]);
    assert.equal(job.with.source_ref, "${{ needs.prepare.outputs.source_sha }}");
  }
  for (const overlap of [undefined, "true", "false"]) {
    const vars = { RELEASE_OVERLAP_FAST_CHECKS: overlap };
    const running = ["quick-checks", "fast-gate"].filter(id => condition(release.jobs[id].if, { needs: { prepare: app }, inputs: {}, vars }));
    assert.deepEqual(running, [overlap === "false" ? "fast-gate" : "quick-checks"], String(overlap));
  }
  const abort = release.jobs["fast-checks-failed"];
  assert.deepEqual(dependencies(abort), ["quick-checks"]);
  for (const result of results) assert.equal(condition(abort.if, { needs: { "quick-checks": { result } } }), result === "failure", result);
  assert.match(abort.steps.map(step => step.run).join("\n"), /gh run cancel \$\{\{ github\.run_id \}\}/);
  for (const id of [...builds, "tests", "quick-checks", "fast-gate"]) assert.notEqual(release.jobs[id]["continue-on-error"], true);
});

test("builds on machines no fast check uses start at once; the rest wait for the fast checks", () => {
  for (const id of overlapped) {
    const job = release.jobs[id];
    assert.deepEqual(dependencies(job), ["prepare", "fast-gate"]);
    for (const gate of results) {
      const needs = { prepare: app, "fast-gate": { result: gate } };
      assert.equal(condition(job.if, { needs }), ["success", "skipped"].includes(gate), `${id}: ${gate}`);
      assert.equal(condition(job.if, { needs, cancelled: true }), false);
    }
    assert.equal(dependencies(job).includes("tests"), false, "builds overlap the Rust tests");
  }
  for (const id of afterFast) {
    const job = release.jobs[id];
    assert.deepEqual(dependencies(job), ["prepare", "quick-checks", "fast-gate"]);
    for (const fast of results) {
      for (const [overlapping, gating] of [[fast, "skipped"], ["skipped", fast]]) {
        const needs = { prepare: app, "quick-checks": { result: overlapping }, "fast-gate": { result: gating } };
        assert.equal(condition(job.if, { needs, inputs: {} }), fast === "success", `${id}: ${overlapping}/${gating}`);
      }
    }
  }
  assert.equal(release.jobs.tests.with.phase, "long");
  assert.deepEqual(new Set(dependencies(release.jobs["release-manifest"])), new Set(["prepare", "quick-checks", "fast-gate", "tests", ...builds]));
});

test("dry runs use both test stages by default; bundle-only diagnostics explicitly bypass them", () => {
  assert.equal(release.on.workflow_dispatch.inputs.dry_run_tests.default, true);
  const base = "(needs.prepare.outputs.dry_run != 'true' || inputs.dry_run_tests) && needs.prepare.outputs.published != 'true'";
  assert.equal(release.jobs["quick-checks"].if, `${base} && vars.RELEASE_OVERLAP_FAST_CHECKS != 'false'`);
  assert.equal(release.jobs["fast-gate"].if, `${base} && vars.RELEASE_OVERLAP_FAST_CHECKS == 'false'`);
  const dry = { result: "success", outputs: { kind: "app", published: "false", dry_run: "true" } };
  for (const id of builds) {
    assert.equal(release.jobs[id].with.dry_run, "${{ needs.prepare.outputs.dry_run == 'true' }}");
    const needs = { prepare: dry, "quick-checks": { result: "skipped" }, "fast-gate": { result: "skipped" } };
    assert.equal(condition(release.jobs[id].if, { needs, inputs: { dry_run_tests: false } }), true, id);
  }
  const draft = release.jobs.prepare.steps.find(step => step.name === "Create draft releases before parallel uploads");
  assert.equal(draft.if, "steps.release.outputs.dry_run != 'true' && steps.release.outputs.published != 'true' && steps.release.outputs.kind == 'app'");
});

test("file sizes gate every other fast check, and test phases partition every check", () => {
  assert.deepEqual(Object.keys(tests.jobs).sort(), ["changed", "rust-warnings", ...fastJobs, ...longJobs].sort());
  const needs = { changed: { result: "success", outputs: { heavy: "true", rust: "true", frontend: "true", plugins: '[{"id":"test"}]' } }, fmt: { result: "success" }, "rust-tests": { result: "success" }, "rust-platform": { result: "success" } };
  for (const id of fastJobs) {
    assert.deepEqual(dependencies(tests.jobs[id]), id === "file-sizes" ? ["changed"] : ["changed", "file-sizes"], id);
    for (const phase of ["all", "fast", "long"]) assert.equal(condition(tests.jobs[id].if, { needs, inputs: { phase } }), ["all", "fast"].includes(phase), `${id}/${phase}`);
  }
  for (const id of longJobs) {
    for (const phase of ["all", "fast", "long"]) assert.equal(condition(tests.jobs[id].if, { needs, inputs: { phase } }), ["all", "long"].includes(phase), `${id}/${phase}`);
  }
  const skipped = { ...needs, fmt: { result: "skipped" } };
  for (const id of ["rust-tests", "rust-platform"]) assert.ok(condition(tests.jobs[id].if, { needs: skipped, inputs: { phase: "long" } }), id);
  assert.match(tests.concurrency.group, /inputs.phase/);
});

test("only a fast-check failure cancels the run; publication needs every stage to succeed", () => {
  const cancelsRun = /actions\/runs\/[^\s"]*\/cancel|gh run cancel/;
  for (const name of ["tests", "release", "rust-warnings-summary"]) {
    for (const [id, job] of Object.entries(workflow(name).jobs)) {
      if (name === "release" && id === "fast-checks-failed") continue;
      for (const step of job.steps || []) assert.doesNotMatch(step.run ?? "", cancelsRun, `${name}/${id}`);
    }
  }
  const vars = { RELEASE_PUBLISH_ENABLED: "true" };
  const ok = Object.fromEntries([...builds, "tests"].map(id => [id, { result: "success" }]));
  const manifest = release.jobs["release-manifest"];
  for (const [overlapping, gating] of [["success", "skipped"], ["skipped", "success"]]) {
    const needs = { ...ok, prepare: app, "quick-checks": { result: overlapping }, "fast-gate": { result: gating } };
    assert.equal(condition(manifest.if, { needs, vars }), true);
    assert.equal(condition(manifest.if, { needs, vars: {} }), false, "publication stays off without the variable");
    for (const id of [...builds, "tests"]) {
      for (const result of ["failure", "cancelled", "skipped"]) {
        assert.equal(condition(manifest.if, { needs: { ...needs, [id]: { result } }, vars }), false, `${id}: ${result}`);
      }
    }
  }
  const failed = { ...ok, prepare: app, "quick-checks": { result: "failure" }, "fast-gate": { result: "skipped" } };
  assert.equal(condition(manifest.if, { needs: failed, vars }), false);
  for (const id of ["release-manifest", "frontend"]) {
    assert.ok(["tests", "quick-checks", "fast-gate"].every(need => dependencies(release.jobs[id]).includes(need)), id);
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
    const run = () => execFileSync("bash", ["-euo", "pipefail", "-c", script], { env: { ...process.env, HEAD_REF: head }, stdio: "pipe" });
    if (ok) run(); else assert.throws(run, undefined, head);
  }
});

test("reusable tests require the release SHA and force scope through both skip gates", () => {
  assert.ok(Object.hasOwn(tests.on, "pull_request"));
  assert.ok(Object.hasOwn(tests.on, "workflow_dispatch"));
  assert.deepEqual(tests.on.schedule, [{ cron: "0 1 * * *" }], "the nightly full matrix on develop");
  assert.equal(tests.on.workflow_call.inputs.source_ref.required, true);
  assert.equal(tests.on.workflow_call.inputs.source_ref.type, "string");
  assert.deepEqual(Object.keys(tests.on.workflow_call.inputs), ["source_ref", "scope", "phase"]);
  const changed = tests.jobs.changed;
  assert.equal(changed.if, "${{ github.event_name != 'pull_request' || !startsWith(github.head_ref, 'release/') }}", "only a release branch's own pull request skips; release calls inherit the push event");
  assert.equal(changed.steps.find(step => step.id === "tree").env.CI_RELEASE_CALL, "${{ inputs.source_ref != '' || github.event_name == 'schedule' }}");
  const scopeEnv = changed.steps.find(step => step.id === "scope").env;
  assert.equal(scopeEnv.CI_FULL_MATRIX, "${{ (inputs.source_ref != '' && inputs.scope != 'frontend') || github.event_name == 'schedule' }}");
  assert.equal(scopeEnv.CI_FRONTEND_ONLY, "${{ inputs.source_ref != '' && inputs.scope == 'frontend' }}");
  for (const job of Object.values(tests.jobs)) {
    for (const step of (job.steps || []).filter(step => step.uses?.startsWith("actions/checkout@"))) {
      assert.equal(step.with.ref, "${{ inputs.source_ref || (github.event_name == 'schedule' && 'refs/heads/develop') || github.ref }}");
    }
  }
});

test("release pushes cannot skip an unchanged tree", () => {
  const script = tests.jobs.changed.steps.find(step => step.id === "tree").run
    .replaceAll("${{ github.event_name }}", "push");
  // A CI runner's stdout can be a socket, where /dev/stdout is not appendable.
  const output = join(mkdtempSync(join(tmpdir(), "release-gate-")), "output");
  execFileSync("bash", ["-euo", "pipefail", "-c", script], {
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
  assert.deepEqual(new Set(dependencies(frontend)), new Set(["prepare", "quick-checks", "fast-gate", "tests"]));
  const frontendRelease = { result: "success", outputs: { kind: "frontend", published: "false", dry_run: "false" } };
  const needs = { prepare: frontendRelease, tests: { result: "success" }, "quick-checks": { result: "success" }, "fast-gate": { result: "skipped" } };
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
    ...["rust-tests", "rust-platform", "clippy-linux", "ts-types"].map(id => tests.jobs[id]),
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
  const full = tests.jobs["rust-tests"];
  assert.deepEqual(full["runs-on"], ["self-hosted", "rust-tests"]);
  assert.deepEqual(full.strategy.matrix.suite, ["full"]);
  const platform = tests.jobs["rust-platform"];
  assert.ok(platform.strategy.matrix.include.every(entry => entry.suite === "platform"));
  assert.deepEqual(platform.strategy.matrix.include.map(entry => entry.os).sort(), ["Linux", "Windows", "macOS"]);
  assert.equal(platform.strategy["fail-fast"], false);
  for (const job of [full, platform]) {
    const runs = job.steps.map(step => step.run ?? "").join(" ; ");
    assert.match(runs, /build\.sh ci-tests \$\{\{ matrix\.suite \}\}/);
    assert.match(runs, /build\.ps1 ci-tests \$\{\{ matrix\.suite \}\}/);
    assert.ok(!job.steps.some(step => step.uses?.startsWith("actions/download-artifact")), "no archive download");
  }
  assert.deepEqual(new Set(dependencies(tests.jobs["rust-receipts"])), new Set(["changed", "rust-tests", "rust-platform"]));
});

test("platform tests expand to one job per OS", () => {
  const { include, ...axes } = tests.jobs["rust-platform"].strategy.matrix;
  assert.deepEqual(Object.keys(axes), [], "added include keys on a base combination overwrite each other");
  assert.deepEqual(include.map(row => [row.suite, row.os]), [["platform", "Linux"], ["platform", "macOS"], ["platform", "Windows"]]);
  for (const row of include) assert.ok(row.label, row.os);
});
