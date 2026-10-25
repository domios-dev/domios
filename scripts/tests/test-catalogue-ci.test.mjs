import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import test from "node:test";
import { parse } from "yaml";
import { scopeForRun } from "../ci-scope.mjs";
import { selectRunners } from "../test-catalogue.mjs";
import { condition } from "./workflow-condition.mjs";

const commandRef = id => `\${{ fromJSON(steps.catalogue.outputs.commands)['${id}'] }}`;
const referencePattern = /\$\{\{ fromJSON\(steps\.catalogue\.outputs\.commands\)\['([^']+)'\] \}\}/g;
const normalize = command => command.trim().replace(/\r\n/g, "\n");

export function prCondition(expression, context = {}) {
  if (expression === undefined) return true;
  const source = String(expression)
    .replaceAll("github.event_name", "'pull_request'")
    .replace(/env\.([\w_]+)/g, (_, key) => JSON.stringify(context.env?.[key] ?? ""));
  const inputs = new Proxy(context.inputs ?? {}, { get: (target, key) => target[key] ?? "" });
  return condition(source, { ...context, inputs });
}

export function prWorkflow(workflows) {
  const caller = workflows["tests-pr.yml"];
  return { ...caller, jobFiles: Object.fromEntries(Object.keys(caller.jobs).map(id => [id, "tests-pr.yml"])) };
}

const ownerOf = (workflow, id) => `${workflow.jobFiles?.[id] ?? "tests.yml"}:${id}`;
const ownsPrJob = (workflow, owner) => {
  const [file, id] = owner.split(":");
  return workflow.jobFiles?.[id] === file;
};

export function prReachability(workflow, files, plugins, results = {}) {
  const gate = workflow.jobs.changed;
  const scopeStep = gate.steps.find(step => step.id === "scope");
  const scope = scopeForRun(files, plugins,
    prCondition(scopeStep.env.CI_FULL_MATRIX), prCondition(scopeStep.env.CI_FAST_ONLY),
    prCondition(scopeStep.env.CI_FRONTEND_ONLY));
  const steps = {
    tree: { outputs: { run: "true" } },
    scope: { outputs: Object.fromEntries(Object.entries(scope).map(([k, v]) => [k, k === "plugins" ? JSON.stringify(v) : String(v)])) },
    "cancel-release": { outcome: "skipped" },
  };
  const outputs = Object.fromEntries(Object.entries(gate.outputs).map(([key, expression]) => [key,
    key === "source_sha" ? "a".repeat(40) : key === "plugins" ? steps.scope.outputs.plugins : String(prCondition(expression, { steps }))]));
  const needs = Object.fromEntries(Object.keys(workflow.jobs).map(id => [id, { result: "skipped", outputs: {} }]));
  const reached = [];
  const pending = new Set(Object.keys(workflow.jobs));
  while (pending.size) {
    let progress = false;
    for (const id of pending) {
      const job = workflow.jobs[id];
      const dependencies = [job.needs ?? []].flat();
      if (dependencies.some(dependency => pending.has(dependency))) continue;
      pending.delete(id);
      progress = true;
      const explicitStatus = /(?:always|cancelled|failure|success)\(\)/.test(job.if ?? "");
      if (!explicitStatus && dependencies.some(dependency => needs[dependency]?.result !== "success")) continue;
      if (!prCondition(job.if, { needs, steps })) continue;
      needs[id] = { result: results[id] ?? "success", outputs: id === "changed" ? outputs : {} };
      reached.push(id);
    }
    if (!progress) throw new Error(`Unresolved workflow dependencies: ${[...pending]}`);
  }
  return { jobs: reached, scope, needs, steps };
}

export function prScenarios(plugins) {
  return [[], ["essences/vision.md"], ["src/app.ts"], ["cli/src/main.rs"],
    ["src/app.ts", "cli/src/main.rs"], ["scripts/unknown.mjs"], ["scripts/check-plugin.mjs"],
    ...plugins.map(plugin => [`plugins/${plugin.path}/plugin.js`])];
}

function runnableSteps(job, context) {
  return (job.steps ?? []).filter(step => step.run &&
    [false, true].some(failed => prCondition(step.if, { ...context, failed, env: { ...job.env, ...step.env } })));
}

export function workflowErrors(workflow, catalogue, overlay, plugins, exceptions, workflows = {}) {
  const errors = new Set();
  const selected = selectRunners(catalogue, overlay, { tier: "pr" }).runners;
  const entries = new Map(catalogue.runners.map(entry => [entry.id, entry]));
  const selectedIds = new Set(selected.map(entry => entry.id));
  if (Object.hasOwn(workflow.on ?? {}, "schedule") && !catalogue.runners.some(entry => entry.id === "qualification-receipt" && entry.ci?.includes("tests.yml:qualification"))) errors.add("schedule trigger has no qualification receipt owner");
  for (const item of exceptions) {
    if (item.job.includes(" → ")) continue;
    const job = workflow.jobs[item.job];
    if (!job || (job.if ?? null) !== item.job_if) errors.add(`${item.job}: change-scoped job condition drift`);
  }
  const projected = selected.filter(entry => entry.ci?.some(owner => ownsPrJob(workflow, owner)));
  for (const files of prScenarios(plugins)) {
    const context = prReachability(workflow, files, plugins);
    const actual = new Set();
    for (const id of context.jobs) {
      const job = workflow.jobs[id];
      if (job.uses) {
        const called = workflows[job.uses.replace("./.github/workflows/", "")];
        if (!called) errors.add(`${id}: unresolved reusable workflow ${job.uses}`);
        for (const [childId, child] of Object.entries(called?.jobs ?? {})) {
          if (!(child["timeout-minutes"] > 0 && child["timeout-minutes"] <= 8)) errors.add(`${id}/${childId}: PR timeout-minutes must be 1..8`);
          for (const step of runnableSteps(child, context)) {
            if (!exceptions.some(item => item.job === `${id} → ${childId}` && item.run === normalize(step.run) && item.if === (step.if ?? null))) errors.add(`${id}/${childId}: unlisted PR command ${step.run}`);
          }
        }
        continue;
      }
      if (!(job["timeout-minutes"] > 0 && job["timeout-minutes"] <= 8)) errors.add(`${id}: PR timeout-minutes must be 1..8`);
      for (const step of runnableSteps(job, context)) {
        for (const entry of catalogue.runners) {
          const directEntry = new RegExp(`run --entry ${entry.id}(?:\\s|$)`).test(step.run);
          const directRunner = normalize(step.run) === entry.runner;
          if ((directEntry || directRunner) && (entry.needs_live_agent || entry.needs_instance || entry.kind === "e2e-plain" || /playwright/i.test(entry.runner))) errors.add(`${id}: forbidden live/instance/Playwright entry ${entry.id}`);
        }
        const refs = [...JSON.stringify({ run: step.run, env: step.env }).matchAll(referencePattern)];
        if (!refs.length) {
          if (!exceptions.some(item => item.job === id && item.run === normalize(step.run) && item.if === (step.if ?? null))) errors.add(`${id}: unlisted PR command ${step.run}`);
          continue;
        }
        const selector = job.steps.find(candidate => candidate.id === "catalogue");
        if (!selector || !exceptions.some(item => item.job === id && item.run === normalize(selector.run))) errors.add(`${id}: missing catalogue select --tier pr wiring`);
        for (const [, entryId] of refs) {
          const entry = entries.get(entryId);
          if (!entry || !selectedIds.has(entryId)) errors.add(`${id}: ${entryId} is not selected by tier pr`);
          if (entry?.needs_live_agent || entry?.needs_instance || entry?.kind === "e2e-plain" || /playwright/i.test(entry?.runner ?? "")) errors.add(`${id}: forbidden live/instance/Playwright entry ${entryId}`);
          if (!entry?.ci?.includes(ownerOf(workflow, id))) errors.add(`${id}: ${entryId} lacks catalogue CI ownership`);
          const recorded = exceptions.find(item => item.job === id && item.entry === entryId);
          if (!recorded || recorded.if !== (step.if ?? null)) errors.add(`${id}: ${entryId} change-scoped step condition drift`);
          const expectedRun = entryId === "ts-generated-types"
            ? `node scripts/collect-rust-warnings.mjs run --log ci-rust-warnings.log -- bash -c '${commandRef(entryId)}'`
            : entryId === "plugin-check" ? '${PLUGIN_COMMAND//<plugin-id>/"$PLUGIN_ID"}' : commandRef(entryId);
          if (normalize(step.run) !== expectedRun) errors.add(`${id}: unlisted PR command around ${entryId}: ${step.run}`);
          actual.add(`${id}:${entryId}`);
        }
      }
    }
    const expected = new Set(projected.flatMap(entry => entry.ci.filter(owner => ownsPrJob(workflow, owner))
      .map(owner => owner.split(":")[1]).filter(id => context.jobs.includes(id))
      .filter(id => {
        const recorded = exceptions.find(item => item.job === id && item.entry === entry.id);
        return recorded?.if == null || [false, true].some(failed => prCondition(recorded.if, { ...context, failed }));
      })
      .map(id => `${id}:${entry.id}`)));
    for (const entry of expected) if (!actual.has(entry)) errors.add(`missing PR catalogue command ${entry} for ${files.join(",") || "docs-only"}`);
    for (const entry of actual) if (!expected.has(entry)) errors.add(`unexpected PR catalogue command ${entry}`);
  }
  return [...errors];
}

export function ownershipErrors(catalogue, overlay, workflows, buildSource, plugins = []) {
  const errors = [];
  const pr = prWorkflow(workflows);
  for (const entry of selectRunners(catalogue, overlay, { tier: "pr" }).runners) {
    if (entry.needs_live_agent || entry.needs_instance || entry.kind === "e2e-plain" || /playwright/i.test(entry.runner)) errors.push(`${entry.id}: forbidden live/instance/Playwright PR catalogue entry`);
    const owners = entry.ci ?? [];
    const valid = owners.some(owner => {
      const [file, id] = owner.split(":");
      const inPr = ownsPrJob(pr, owner);
      const workflow = inPr ? pr : workflows[file];
      const job = workflow?.jobs?.[id];
      if (!workflow?.on || !("pull_request" in workflow.on) || !job) return false;
      const contexts = inPr ? prScenarios(plugins).map(files => prReachability(workflow, files, plugins)) : [{}];
      const reachable = contexts.filter(context => inPr ? context.jobs.includes(id) : prCondition(job.if, context));
      if (!reachable.length) return false;
      const steps = reachable.flatMap(context => runnableSteps(job, context));
      const commands = steps.map(step => normalize(step.run));
      if (steps.some(step => JSON.stringify(step).includes(commandRef(entry.id)))) return true;
      const combined = commands.join("\n");
      if (entry.runner.split(" && ").every(command => combined.includes(command.replace("./build.sh", "bash build.sh")))) return true;
      if (entry.id === "provider-boundary" && combined.includes(entry.runner)) return true;
      return false;
    });
    if (!valid) errors.push(`${entry.id}: no pull_request workflow executes this tier-pr entry via its ci owners`);
  }
  return errors;
}

const read = relative => readFileSync(new URL(`../../${relative}`, import.meta.url), "utf8");
const catalogue = JSON.parse(read("tests/catalogue.json"));
const overlay = JSON.parse(read("tests/catalogue.overlay.json"));
const plugins = JSON.parse(read("plugins/channel.json")).plugins;
const workflows = Object.fromEntries(readdirSync(new URL("../../.github/workflows/", import.meta.url))
  .filter(file => file.endsWith(".yml")).map(file => [file, parse(read(`.github/workflows/${file}`))]));
const workflow = prWorkflow(workflows);
const exceptions = JSON.parse(read("docs/test-strategy/pr-commands.md").match(/```json\r?\n([\s\S]*?)\r?\n```/)[1]);
const check = (value, cat = catalogue, all = workflows) => workflowErrors(value, cat, overlay, plugins, exceptions, all);

test("PR commands equal the catalogue projection for each ci-scope change class", () => {
  assert.deepEqual(check(workflow), []);
});

test("every selected PR entry has an executing PR workflow owner", () => {
  assert.deepEqual(ownershipErrors(catalogue, overlay, workflows, read("build.sh"), plugins), []);
});

test("fast checks run alongside required guards and retain failure cancellation", () => {
  const fast = workflows["tests-fast.yml"];
  const cancel = fast.jobs.guards.steps.find(step => step.name === "Notify and cancel on first failure");
  assert.equal(cancel.if, "failure() && inputs.cancel_run");
  assert.match(cancel.run, /gh run cancel/);
  assert.equal(fast.jobs.guards["continue-on-error"], undefined);
  assert.match(fast.jobs.guards.if, /github.actor == 'rollacode'/);
  for (const [id, job] of Object.entries(fast.jobs)) {
    if (id === "changed") continue;
    assert.deepEqual([job.needs].flat(), ["changed"], id);
    assert.equal(job["continue-on-error"], undefined, id);
    assert.ok(job.steps.some(step => step.run === cancel.run && step.if === cancel.if), id);
  }
  const reached = prReachability(workflow, ["src/app.ts"], plugins, { guards: "failure" });
  assert.equal(reached.needs.guards.result, "failure");
  for (const id of ["ts-static", "ts-unit", "ts-vitest", "frontend-windows", "ts-types"]) {
    assert.ok(reached.jobs.includes(id), id);
  }
  assert.equal(workflows["tests.yml"].jobs.fast.uses, "./.github/workflows/tests-fast.yml");
  assert.equal(workflows["tests.yml"].jobs.fast["continue-on-error"], undefined);
  assert.equal(workflows["tests.yml"].jobs.rust.needs, "fast");
  assert.ok(workflows["release.yml"].jobs.prepare.steps.some(step => /ci-qualification/.test(step.run ?? "")));
});

test("Windows retains focused stable-path TS2451 coverage; the frontend producer owns the full build", () => {
  const job = workflows["tests-fast.yml"].jobs["frontend-windows"];
  assert.ok(job.steps.some(step => step.run === commandRef("frontend-windows-build")));
  assert.equal(catalogue.runners.find(entry => entry.id === "frontend-windows-build").runner, "node --test scripts/tests/windows-path-identity.test.mjs");
  assert.equal(catalogue.runners.find(entry => entry.id === "ts-typecheck").runner, "pnpm build");
  assert.ok(catalogue.runners.find(entry => entry.id === "ts-typecheck").ci.includes("tests-pr.yml:frontend-dist"));
});


test("heavy Rust jobs are not PR-reachable, including a Rust change", t => {
  const reached = prReachability(workflow, ["cli/src/main.rs"], plugins);
  assert.equal(reached.scope.heavy, false);
  for (const id of ["rust-tests", "rust-platform", "rust-receipts", "clippy-linux"]) assert.ok(!reached.jobs.includes(id), id);
  assert.ok(reached.jobs.includes("guards"));
  t.diagnostic(`GREEN heavy=false; PR jobs for Rust: ${reached.jobs.join(", ")}`);
});

test("red demo: an unlisted PR command is rejected", t => {
  const copy = structuredClone(workflow);
  copy.jobs.guards.steps.push({ run: "echo unlisted" });
  const errors = check(copy);
  assert.ok(errors.some(error => error.includes("unlisted PR command echo unlisted")));
  t.diagnostic(`RED ${errors[0]}`);
});

test("red demos: live, instance and Playwright entries cannot run on PR", () => {
  for (const policy of [{ needs_live_agent: true }, { needs_instance: true }, { kind: "e2e-plain" }, { runner: "pnpm exec playwright test" }]) {
    const copy = structuredClone(catalogue);
    Object.assign(copy.runners.find(entry => entry.id === "ts-unit"), policy);
    assert.ok(check(workflow, copy).some(error => error.includes("forbidden live/instance/Playwright entry ts-unit")));
  }
  const copy = structuredClone(workflow);
  copy.jobs.guards.steps.push({ run: "node scripts/test-catalogue.mjs run --entry e2e-api" });
  assert.ok(check(copy).some(error => error.includes("forbidden live/instance/Playwright entry e2e-api")));
});

test("red demos: missing and excessive PR timeouts are rejected", t => {
  for (const timeout of [undefined, 20]) {
    const copy = structuredClone(workflow);
    copy.jobs.guards["timeout-minutes"] = timeout;
    const errors = check(copy);
    assert.ok(errors.some(error => error.includes("guards: PR timeout-minutes")));
    t.diagnostic(`RED timeout=${timeout}: ${errors[0]}`);
  }
  assert.equal(workflow.jobs["rust-warnings"], undefined, "private warning storage is not PR-reachable");
});

test("red demo: a schedule trigger requires a declared scheduled tier", t => {
  const copy = structuredClone(workflow);
  copy.on.schedule = [{ cron: "0 0 * * *" }];
  const noScheduled = structuredClone(catalogue);
  noScheduled.runners = noScheduled.runners.filter(entry => entry.id !== "qualification-receipt");
  const errors = check(copy, noScheduled);
  assert.ok(errors.some(error => error.includes("schedule trigger")));
  t.diagnostic(`RED ${errors[0]}`);
});

test("red demos: widening a job scope or changing step conditions is rejected", () => {
  const copy = structuredClone(workflow);
  delete copy.jobs["ts-unit"].if;
  assert.ok(check(copy).some(error => error.includes("ts-unit: change-scoped job condition drift")));
  const steps = structuredClone(workflow);
  delete steps.jobs["ts-static"].steps.find(step => step.run === commandRef("ts-unused-exports")).if;
  assert.ok(check(steps).some(error => error.includes("change-scoped step condition drift")));
});

test("red demos: missing catalogue wiring, orphan entries and removed commands fail", () => {
  const copy = structuredClone(workflow);
  copy.jobs["ts-unit"].steps = copy.jobs["ts-unit"].steps.filter(step => step.run !== commandRef("ts-unit"));
  assert.ok(check(copy).some(error => error.includes("missing PR catalogue command ts-unit:ts-unit")));
  const cat = structuredClone(catalogue);
  cat.runners.push({ id: "orphan", tier: "pr", runner: "echo orphan" });
  assert.ok(ownershipErrors(cat, overlay, workflows, "").some(error => error.startsWith("orphan:")));
});
