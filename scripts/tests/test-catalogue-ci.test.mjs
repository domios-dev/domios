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

export function prReachability(workflow, files, plugins) {
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
    key === "plugins" ? steps.scope.outputs.plugins : String(prCondition(expression, { steps }))]));
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
      needs[id] = { result: "success", outputs: id === "changed" ? outputs : {} };
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
  if (Object.hasOwn(workflow.on ?? {}, "schedule") && !catalogue.runners.some(entry => entry.tier === "scheduled")) errors.add("schedule trigger has no scheduled catalogue tier");
  for (const item of exceptions) {
    if (item.job.includes(" → ")) continue;
    const job = workflow.jobs[item.job];
    if (!job || (job.if ?? null) !== item.job_if) errors.add(`${item.job}: change-scoped job condition drift`);
  }
  const projected = selected.filter(entry => entry.ci?.some(owner => owner.startsWith("tests.yml:")));
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
          if (!entry?.ci?.includes(`tests.yml:${id}`)) errors.add(`${id}: ${entryId} lacks catalogue CI ownership`);
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
    const expected = new Set(projected.flatMap(entry => entry.ci.filter(owner => owner.startsWith("tests.yml:"))
      .map(owner => owner.slice("tests.yml:".length)).filter(id => context.jobs.includes(id))
      .filter(id => !(id === "plugin-metadata" && entry.id === "node-script-tests"))
      .map(id => `${id}:${entry.id}`)));
    for (const entry of expected) if (!actual.has(entry)) errors.add(`missing PR catalogue command ${entry} for ${files.join(",") || "docs-only"}`);
    for (const entry of actual) if (!expected.has(entry)) errors.add(`unexpected PR catalogue command ${entry}`);
  }
  return [...errors];
}

export function ownershipErrors(catalogue, overlay, workflows, buildSource, plugins = []) {
  const errors = [];
  for (const entry of selectRunners(catalogue, overlay, { tier: "pr" }).runners) {
    if (entry.needs_live_agent || entry.needs_instance || entry.kind === "e2e-plain" || /playwright/i.test(entry.runner)) errors.push(`${entry.id}: forbidden live/instance/Playwright PR catalogue entry`);
    const owners = entry.ci ?? [];
    const valid = owners.some(owner => {
      const [file, id] = owner.split(":");
      const workflow = workflows[file];
      const job = workflow?.jobs?.[id];
      if (!workflow?.on || !("pull_request" in workflow.on) || !job) return false;
      const contexts = file === "tests.yml" ? prScenarios(plugins).map(files => prReachability(workflow, files, plugins)) : [{}];
      const reachable = contexts.filter(context => file === "tests.yml" ? context.jobs.includes(id) : prCondition(job.if, context));
      if (!reachable.length) return false;
      const steps = reachable.flatMap(context => runnableSteps(job, context));
      const commands = steps.map(step => normalize(step.run));
      if (steps.some(step => JSON.stringify(step).includes(commandRef(entry.id)))) return true;
      const combined = commands.join("\n");
      if (entry.runner.split(" && ").every(command => combined.includes(command.replace("./build.sh", "bash build.sh")))) return true;
      if (entry.id === "provider-boundary" && combined.includes(entry.runner)) return true;
      if (["node-script-tests-serial", "node-script-tests-cargo", "script-catalog", "test-catalogue"].includes(entry.id)) {
        return combined.includes("bash build.sh script-guards") && buildSource.includes(entry.runner);
      }
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
const workflow = workflows["tests.yml"];
const exceptions = JSON.parse(read("docs/test-strategy/pr-commands.md").match(/```json\n([\s\S]*?)\n```/)[1]);
const check = (value, cat = catalogue, all = workflows) => workflowErrors(value, cat, overlay, plugins, exceptions, all);

test("PR commands equal the catalogue projection for each ci-scope change class", () => {
  assert.deepEqual(check(workflow), []);
});

test("every selected PR entry has an executing PR workflow owner", () => {
  assert.deepEqual(ownershipErrors(catalogue, overlay, workflows, read("build.sh"), plugins), []);
});

test("heavy Rust jobs are not PR-reachable, including a Rust change", t => {
  const reached = prReachability(workflow, ["cli/src/main.rs"], plugins);
  assert.equal(reached.scope.heavy, false);
  for (const id of ["rust-archives", "rust-shards", "rust-platform", "rust-input-platform", "rust-coverage", "clippy-linux"]) assert.ok(!reached.jobs.includes(id), id);
  assert.ok(reached.jobs.includes("fmt"));
  assert.ok(reached.jobs.includes("release-contracts"));
  t.diagnostic(`GREEN heavy=false; PR jobs for Rust: ${reached.jobs.join(", ")}`);
});

test("red demo: an unlisted PR command is rejected", t => {
  const copy = structuredClone(workflow);
  copy.jobs["plugin-metadata"].steps.push({ run: "echo unlisted" });
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
  copy.jobs["plugin-metadata"].steps.push({ run: "node scripts/test-catalogue.mjs run --entry e2e-api" });
  assert.ok(check(copy).some(error => error.includes("forbidden live/instance/Playwright entry e2e-api")));
});

test("red demos: missing and excessive PR timeouts are rejected", t => {
  for (const timeout of [undefined, 20]) {
    const copy = structuredClone(workflow);
    copy.jobs["plugin-metadata"]["timeout-minutes"] = timeout;
    const errors = check(copy);
    assert.ok(errors.some(error => error.includes("plugin-metadata: PR timeout-minutes")));
    t.diagnostic(`RED timeout=${timeout}: ${errors[0]}`);
  }
  const copy = structuredClone(workflows);
  copy["rust-warnings-summary.yml"].jobs.report["timeout-minutes"] = 20;
  assert.ok(check(workflow, catalogue, copy).some(error => error.includes("rust-warnings/report: PR timeout-minutes")));
});

test("red demo: a schedule trigger requires a declared scheduled tier", t => {
  const copy = structuredClone(workflow);
  copy.on.schedule = [{ cron: "0 0 * * *" }];
  const errors = check(copy);
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
