import {bashInterpreter} from "../../scripts/bash-interpreter.mjs";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { main, parseArgs, persistedSelection, preferred } from "./driver.mjs";
import { loadAreas, selectionFromFlags } from "./lib/selection.mjs";
import { assertIsolated, target } from "./lib/env.mjs";
import { PHASE_STEPS, buildPlan, planFromEnv, renderPlan } from "./lib/plan.mjs";
import { parseRestartArgs, restartEnvironment } from "./restart.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const runSh = join(here, "run.sh");
const bash = process.env.CORE_JOURNEY_BASH ?? bashInterpreter();

test("parseArgs reads driver flags and refuses unknown or missing ones", () => {
  const options = parseArgs(["verify", "--out", "/tmp/x", "--providers", "claude, codex", "--history-kb", "8", "--all-tabs"]);
  assert.equal(options.mode, "verify");
  assert.deepEqual(options.providers, ["claude", "codex"]);
  assert.equal(options.historyBytes, 8192);
  assert.ok(options.allTabs);
  assert.deepEqual({ ...parseArgs(["setup", "--out", "/tmp/x", "--model", "acp=gemini-cli", "--model", "codex=opaque/model"]).models },
    { acp: "gemini-cli", codex: "opaque/model" });
  assert.throws(() => parseArgs(["verify"]), /--out/);
  assert.throws(() => parseArgs(["deploy", "--out", "/tmp/x"]), /mode/);
  assert.throws(() => parseArgs(["setup", "--out", "/tmp/x", "--bogus"]), /flag/);
});

test("preferred picks by capability, ranking the required providers first", () => {
  const run = [{ id: "aider", runtime: false }, { id: "claude", runtime: false }, { id: "codex", runtime: true }];
  assert.equal(preferred(run, false).id, "claude");
  assert.equal(preferred(run, true).id, "codex");
  assert.equal(preferred([{ id: "claude", runtime: false }], true), null);
});

test("parseRestartArgs requires an output directory and refuses unknown flags", () => {
  assert.deepEqual(parseRestartArgs(["--out", "/tmp/r"]), {out: resolve("/tmp/r")});
  assert.throws(() => parseRestartArgs([]), /--out/);
  assert.throws(() => parseRestartArgs(["--out", "/tmp/r", "--unknown", "value"]), /unknown restart flag/);
});

test("restart retains the selected window ports and scrubs inherited connection and authority", () => {
  const identity = { cli: { instance: "CODETERM_INSTANCE" },
    request: { instance: "CODETERM_INSTANCE", port: "CODETERM_API_PORT" },
    development: { instance: "CODETERM_DEV_INSTANCE", port: "CODETERM_DEV_API_PORT" } };
  const dev = { instance: "curated", port: 7697 };
  const launch = { name: "curated", api_port: "7697", vite_port: "14997", cdp_port: "9397" };
  const env = restartEnvironment(identity, dev, launch, { CODETERM_API_PORT: "7685",
    CODETERM_API_TOKEN: "foreign", CODETERM_SESSION_ID: "foreign", CODETERM_TAB_ID: "foreign",
    CODETERM_DEV_CDP_PORT: "9223", PATH: "retained" });
  assert.deepEqual(env, { PATH: "retained", CODETERM_INSTANCE: "curated", CODETERM_API_PORT: "7697",
    CODETERM_DEV_INSTANCE: "curated", CODETERM_DEV_API_PORT: "7697",
    CODETERM_DEV_VITE_PORT: "14997", CODETERM_DEV_CDP_PORT: "9397" });
  assert.throws(() => restartEnvironment(identity, dev, { ...launch, name: "foreign" }), /differs/);
  assert.throws(() => restartEnvironment(identity, dev, { ...launch, cdp_port: "" }), /original DEV/);
});

const connection = (extra) => ({ CORE_JOURNEY_DATA_DIR: "/tmp/dev", CORE_JOURNEY_API_TOKEN: "t", ...extra });

test("target refuses production, the shared DEV port and an unset instance", () => {
  assert.equal(target(connection({ CORE_JOURNEY_INSTANCE: "curated", CORE_JOURNEY_API_PORT: "7697" })).port, 7697);
  assert.throws(() => target(connection({ CORE_JOURNEY_INSTANCE: "curated", CORE_JOURNEY_API_PORT: "7685" })), /7685 is production/);
  assert.throws(() => target(connection({ CORE_JOURNEY_INSTANCE: "curated", CORE_JOURNEY_API_PORT: "7686" })), /7686 is the shared DEV/);
  assert.throws(() => target(connection({ CORE_JOURNEY_API_PORT: "7697" })), /named instance is unset/);
  assert.throws(() => target(connection({ CORE_JOURNEY_INSTANCE: "  ", CORE_JOURNEY_API_PORT: "7697" })), /named instance is unset/);
  assert.throws(() => assertIsolated({ instance: "x", port: 7685 }), /production/);
});

test("--plan prints the plan from the driver without an instance and still refuses a shared target", async () => {
  const written = [];
  const original = process.stdout.write.bind(process.stdout);
  const saved = { ...process.env };
  Object.assign(process.env, { CORE_JOURNEY_INSTANCE: "curated", CORE_JOURNEY_API_PORT: "7697" });
  process.stdout.write = (chunk) => { written.push(String(chunk)); return true; };
  try {
    assert.equal(await main(["verify", "--out", join(here, "never-created"), "--plan"]), 0);
    process.env.CORE_JOURNEY_API_PORT = "7686";
    await assert.rejects(main(["setup", "--out", join(here, "never-created"), "--plan"]), /7686/);
  } finally {
    process.stdout.write = original;
    for (const key of Object.keys(process.env)) if (!(key in saved)) delete process.env[key];
    Object.assign(process.env, saved);
  }
  assert.match(written.join(""), /PLAN verify[^]*restored_tab_ready[^]*screenshots.verify[.]png/);
});

test("the plan lists every stage, step and evidence path of each mode", () => {
  const plan = buildPlan({ mode: "all", out: "/o", instance: "curated", port: 7697 });
  assert.deepEqual(plan.stages.map((stage) => stage.phase), ["setup", "restart", "verify", "cleanup"]);
  assert.deepEqual(plan.evidence, ["/o/results.json", "/o/results.md", "/o/restart.json", "/o/screenshots/setup.png", "/o/screenshots/verify.png"]);
  assert.match(renderPlan(plan), /target: instance=curated port=7697[^]*evidence-contract[.]mjs/);
  assert.throws(() => buildPlan({ mode: "deploy", out: "/o", instance: "x" }), /unknown plan mode/);
  assert.throws(() => planFromEnv({ mode: "all", out: "/o", instance: "", port: "7697" }), /unset/);
});

const j1Selection = (areas) => selectionFromFlags(areas, parseArgs(["setup", "--out", "/o", "--journeys", "J1"]).selection);

test("selection flags are parsed verbatim; only a J4 selection plans a restart, and J4 alone plans its own setup", () => {
  const options = parseArgs(["setup", "--out", "/o", "--journeys", "J4", "--exclude-provider", "acp", "--waiver", "owner: acp quota"]);
  assert.deepEqual(options.selection, { journeys: "J4", selection: null, exclude_providers: ["acp"], waiver: "owner: acp quota" });
  assert.throws(() => parseArgs(["setup", "--out", "/o", "--waiver", "a", "--waiver", "b"]), /given twice/);
  const areas = loadAreas();
  const steps = (plan) => plan.stages.map((stage) => `${stage.phase}:${stage.steps.map((step) => `${step.journey}/${step.check}`).join(",")}`);
  const j4 = buildPlan({ mode: "all", out: "/o", instance: "curated", port: 7697, selection: selectionFromFlags(areas, parseArgs(["setup", "--out", "/o", "--journeys", "J4"]).selection) });
  assert.deepEqual(j4.stages.map((stage) => stage.phase), ["setup", "restart", "verify", "cleanup"]);
  assert.match(steps(j4)[0], /J4\/restore_fixture_ready.*J4\/pre_restart_snapshot/);
  assert.doesNotMatch(steps(j4).join(), /J1\/|J2\/|J3\/|restored_worker_attempt_binding/);
  const j1 = buildPlan({ mode: "all", out: "/o", instance: "curated", port: 7697, selection: selectionFromFlags(areas, parseArgs(["setup", "--out", "/o", "--journeys", "J1,J3"]).selection) });
  assert.deepEqual(j1.stages.map((stage) => stage.phase), ["setup", "cleanup"]);
  assert.match(steps(j1)[0], /J3\/review_accept_done/);
  assert.deepEqual(j1.evidence, ["/o/results.json", "/o/results.md", "/o/screenshots/setup.png"]);
  assert.match(renderPlan(j1), /journey J2: not run \(1 reason: not in --journeys\)/);
  assert.throws(() => buildPlan({ mode: "verify", out: "/o", instance: "curated", selection: j1Selection(areas) }), /no restart/);
});

test("setup records the selection; verify must repeat it and needs J4", () => {
  const areas = loadAreas(), full = selectionFromFlags(areas, parseArgs(["setup", "--out", "/o"]).selection);
  const results = {};
  persistedSelection("setup", results, full);
  assert.equal(results.selection, full);
  assert.doesNotThrow(() => persistedSelection("verify", results, structuredClone(full)));
  assert.throws(() => persistedSelection("setup", results, j1Selection(areas)), /different journey selection/);
  assert.throws(() => persistedSelection("verify", results, j1Selection(areas)), /repeat the setup selection flags/);
  assert.throws(() => persistedSelection("verify", { selection: j1Selection(areas) }, j1Selection(areas)), /no restart to verify/);
  assert.throws(() => persistedSelection("verify", {}, full), /selection recorded by setup/);
});

test("every planned check name exists in the journey sources", () => {
  const files = [join(here, "driver.mjs"), join(here, "lib/screenshots.mjs"),
    ...readdirSync(join(here, "journeys")).map((name) => join(here, "journeys", name))];
  const source = files.map((file) => readFileSync(file, "utf8")).join("\n");
  for (const [phase, groups] of Object.entries(PHASE_STEPS)) {
    for (const [journey, checks] of groups) {
      for (const check of checks) {
        const name = check.split(" ")[0];
        assert.ok(source.includes(`"${name}"`), `${phase} ${journey}: no check named ${name} in the journey sources`);
      }
    }
  }
});

function runner(args, env) {
  const clean = { ...process.env };
  const previousProductPrefix = ["CODE", "TERM"].join("");
  for (const key of Object.keys(clean)) if (key.startsWith(`${previousProductPrefix}_`)) delete clean[key];
  return spawnSync(bash, [runSh, ...args], { encoding: "utf8", env: { ...clean, ...env }, timeout: 60_000 });
}

const previousInstanceEnv = `${["CODE", "TERM"].join("")}_INSTANCE`;
const previousPortEnv = `${["CODE", "TERM"].join("")}_API_PORT`;
const previousTarget = (instance, port) => ({ [previousInstanceEnv]: instance, [previousPortEnv]: port });

test("run.sh --plan exits 0 on a named instance and prints steps and evidence paths", () => {
  const result = runner(["all", "--plan"], previousTarget("curated", "7697"));
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /PLAN all[^]*J1: [^]*spawn_ready[^]*results[.]json[^]*restart[.]json[^]*setup[.]png/);
  const narrowed = runner(["all", "--plan", "--journeys", "J1"], previousTarget("curated", "7697"));
  assert.equal(narrowed.status, 0, narrowed.stderr);
  assert.match(narrowed.stdout, /journey J4: not run/);
  assert.doesNotMatch(narrowed.stdout, /stage \d: restart/);
  const unwaived = runner(["all", "--plan", "--exclude-provider", "acp"], previousTarget("curated", "7697"));
  assert.notEqual(unwaived.status, 0);
  assert.match(unwaived.stderr, /--waiver/);
});

test("run.sh refuses ports 7685 and 7686 and an unset instance, in plan and live modes", () => {
  // Every refusal in one live mode; each other mode proves it shares the same pre-dispatch guard.
  const cases = [
    [["all"], previousTarget("curated", "7685"), /production port 7685/],
    [["all"], previousTarget("curated", "7686"), /shared DEV port 7686/],
    [["all"], { [previousPortEnv]: "7697" }, /named instance is unset/],
    [["all"], {}, /named instance is unset/],
    [["all", "--plan"], previousTarget("curated", "7685"), /production port 7685/],
    [["setup"], {}, /named instance is unset/],
  ];
  for (const [mode, env, message] of cases) {
    const result = runner(mode, env);
    assert.notEqual(result.status, 0, `${mode.join(" ")} ${JSON.stringify(env)} must be refused`);
    assert.match(result.stderr, message);
  }
});
