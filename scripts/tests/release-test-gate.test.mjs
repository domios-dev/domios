import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { parse } from "yaml";

function workflow(name) {
  return parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), "utf8"));
}

const release = workflow("release");
const tests = workflow("tests");
const builds = ["macos", "linux-arm64", "linux-x86_64", "windows"];
const dependencies = job => [job.needs].flat();

test("release tests and platform builds share preparation and publication waits for every gate", () => {
  assert.equal(release.jobs.tests.uses, "./.github/workflows/tests.yml");
  assert.deepEqual(release.jobs.tests.with, { source_ref: "${{ needs.prepare.outputs.source_sha }}", scope: "${{ needs.prepare.outputs.kind == 'app' && 'full' || 'frontend' }}" });
  for (const id of ["tests", ...builds]) {
    assert.deepEqual(dependencies(release.jobs[id]), ["prepare"]);
    assert.notEqual(release.jobs[id]["continue-on-error"], true);
  }
  assert.deepEqual(new Set(dependencies(release.jobs["release-manifest"])), new Set(["prepare", "tests", ...builds]));
  assert.equal(release.jobs["release-manifest"].if, "needs.prepare.outputs.dry_run != 'true' && needs.prepare.outputs.published != 'true' && needs.prepare.outputs.kind == 'app'");
});

test("dry runs test every platform by default while skipping publication", () => {
  assert.equal(release.on.workflow_dispatch.inputs.dry_run_tests.default, true);
  assert.equal(release.jobs.tests.if, "(needs.prepare.outputs.dry_run != 'true' || inputs.dry_run_tests) && needs.prepare.outputs.published != 'true'");
  for (const id of builds) {
    assert.equal(release.jobs[id].with.dry_run, "${{ needs.prepare.outputs.dry_run == 'true' }}");
    assert.equal(release.jobs[id].if, "needs.prepare.outputs.published != 'true' && needs.prepare.outputs.kind == 'app'");
  }
  const draft = release.jobs.prepare.steps.find(step => step.name === "Create draft releases before parallel uploads");
  assert.equal(draft.if, "steps.release.outputs.dry_run != 'true' && steps.release.outputs.published != 'true' && steps.release.outputs.kind == 'app'");
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
  assert.equal(tests.on.workflow_call.inputs.source_ref.required, true);
  assert.equal(tests.on.workflow_call.inputs.source_ref.type, "string");
  assert.deepEqual(Object.keys(tests.on.workflow_call.inputs), ["source_ref", "scope"]);
  const changed = tests.jobs.changed;
  assert.equal(changed.if, undefined, "the scope gate runs for every trigger, including release calls");
  assert.equal(changed.steps.find(step => step.id === "tree").env.CI_RELEASE_CALL, "${{ inputs.source_ref != '' || github.event_name == 'schedule' }}");
  const scopeEnv = changed.steps.find(step => step.id === "scope").env;
  assert.equal(scopeEnv.CI_FULL_MATRIX, "${{ (inputs.source_ref != '' && inputs.scope != 'frontend') || github.event_name == 'schedule' }}");
  assert.equal(scopeEnv.CI_FRONTEND_ONLY, "${{ inputs.source_ref != '' && inputs.scope == 'frontend' }}");
  for (const job of Object.values(tests.jobs)) {
    for (const step of job.steps.filter(step => step.uses?.startsWith("actions/checkout@"))) {
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
  for (const name of ["release-builder", "release-linux", "release-windows"]) {
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
  assert.deepEqual(new Set(dependencies(frontend)), new Set(["prepare", "tests"]));
  assert.equal(frontend.if, "needs.prepare.outputs.kind == 'frontend' && needs.prepare.outputs.published != 'true' && needs.prepare.outputs.dry_run != 'true'");
  assert.deepEqual(frontend.with, { tag: "${{ needs.prepare.outputs.tag }}", source_sha: "${{ needs.prepare.outputs.source_sha }}" });
  const resolve = release.jobs.prepare.steps.find(step => step.id === "release").run;
  assert.match(resolve, /kind=\$\(node scripts\/ci-scope\.mjs --release-kind "\$previous" "\$sha"\)/);
  assert.match(resolve, /tag="frontend-v\$version"/);
  const called = workflow("frontend-release");
  assert.deepEqual(Object.keys(called.on.workflow_call.inputs), ["tag", "source_sha"]);
  assert.equal(called.on.push, undefined, "a frontend tag push no longer publishes outside the release run");
});
