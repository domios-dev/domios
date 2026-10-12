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
const builds = ["macos-linux-arm64", "linux-x86_64", "windows"];
const dependencies = job => [job.needs].flat();

test("release tests and platform builds share preparation and publication waits for every gate", () => {
  assert.equal(release.jobs.tests.uses, "./.github/workflows/tests.yml");
  assert.deepEqual(release.jobs.tests.with, { source_ref: "${{ needs.prepare.outputs.source_sha }}" });
  for (const id of ["tests", ...builds]) {
    assert.deepEqual(dependencies(release.jobs[id]), ["prepare"]);
    assert.notEqual(release.jobs[id]["continue-on-error"], true);
  }
  assert.deepEqual(new Set(dependencies(release.jobs["release-manifest"])), new Set(["prepare", "tests", ...builds]));
  assert.equal(release.jobs["release-manifest"].if, "needs.prepare.outputs.dry_run != 'true'");
});

test("dry runs still build every platform while skipping the tests and publisher", () => {
  assert.equal(release.jobs.tests.if, "needs.prepare.outputs.dry_run != 'true'");
  for (const id of builds) {
    assert.equal(release.jobs[id].with.dry_run, "${{ needs.prepare.outputs.dry_run == 'true' }}");
    assert.equal(release.jobs[id].if, undefined);
  }
  const draft = release.jobs.prepare.steps.find(step => step.name === "Create draft releases before parallel uploads");
  assert.equal(draft.if, "steps.release.outputs.dry_run != 'true'");
});

test("reusable tests require the release SHA and force scope through both skip gates", () => {
  assert.ok(Object.hasOwn(tests.on, "pull_request"));
  assert.ok(Object.hasOwn(tests.on, "workflow_dispatch"));
  assert.equal(tests.on.workflow_call.inputs.source_ref.required, true);
  assert.equal(tests.on.workflow_call.inputs.source_ref.type, "string");
  assert.deepEqual(Object.keys(tests.on.workflow_call.inputs), ["source_ref"]);
  const changed = tests.jobs.changed;
  assert.equal(changed.if, undefined, "the scope gate runs for every trigger, including release calls");
  for (const id of ["tree", "scope"]) {
    assert.equal(changed.steps.find(step => step.id === id).env.CI_FULL_MATRIX, "${{ inputs.source_ref != '' }}");
  }
  for (const job of Object.values(tests.jobs)) {
    for (const step of job.steps.filter(step => step.uses?.startsWith("actions/checkout@"))) {
      assert.equal(step.with.ref, "${{ inputs.source_ref || github.ref }}");
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
    env: { ...process.env, CI_FULL_MATRIX: "true", GITHUB_OUTPUT: output },
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
