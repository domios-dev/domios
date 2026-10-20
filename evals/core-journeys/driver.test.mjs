import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { main, parseArgs, preferred } from "./driver.mjs";
import { assertIsolated, target } from "./lib/env.mjs";
import { PHASE_STEPS, buildPlan, planFromEnv, renderPlan } from "./lib/plan.mjs";
import { parseRestartArgs } from "./restart.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const runSh = join(here, "run.sh");

test("parseArgs reads driver flags and refuses unknown or missing ones", () => {
  const options = parseArgs(["verify", "--out", "/tmp/x", "--providers", "claude, codex", "--history-kb", "8", "--all-tabs", "--expect-build-change"]);
  assert.equal(options.mode, "verify");
  assert.deepEqual(options.providers, ["claude", "codex"]);
  assert.equal(options.historyBytes, 8192);
  assert.ok(options.allTabs && options.expectBuildChange);
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

test("parseRestartArgs defaults to a prebuilt restart and accepts the upgrade shape", () => {
  assert.equal(parseRestartArgs(["--out", "/tmp/r"]).start, "dev-prebuilt");
  const upgrade = parseRestartArgs(["--out", "/tmp/r", "--stop-in", "/tmp/old", "--start", "dev", "--label", "upgrade"]);
  assert.equal(upgrade.stopIn, resolve("/tmp/old"));
  assert.equal(upgrade.label, "upgrade");
  assert.throws(() => parseRestartArgs(["--out", "/tmp/r", "--start", "build"]), /--start/);
});

const connection = (extra) => ({ CODETERM_DEV_DATA_DIR: "/tmp/dev", CODETERM_API_TOKEN: "t", ...extra });

test("target refuses production, the shared DEV port and an unset instance", () => {
  assert.equal(target(connection({ CODETERM_INSTANCE: "curated", CODETERM_API_PORT: "7697" })).port, 7697);
  assert.throws(() => target(connection({ CODETERM_INSTANCE: "curated", CODETERM_API_PORT: "7685" })), /7685 is production/);
  assert.throws(() => target(connection({ CODETERM_INSTANCE: "curated", CODETERM_API_PORT: "7686" })), /7686 is the shared DEV/);
  assert.throws(() => target(connection({ CODETERM_API_PORT: "7697" })), /CODETERM_INSTANCE is unset/);
  assert.throws(() => target(connection({ CODETERM_INSTANCE: "  ", CODETERM_API_PORT: "7697" })), /CODETERM_INSTANCE is unset/);
  assert.throws(() => assertIsolated({ instance: "x", port: 7685 }), /production/);
});

test("--plan prints the plan from the driver without an instance and still refuses a shared target", async () => {
  const written = [];
  const original = process.stdout.write.bind(process.stdout);
  const saved = { ...process.env };
  Object.assign(process.env, { CODETERM_INSTANCE: "curated", CODETERM_API_PORT: "7697" });
  process.stdout.write = (chunk) => { written.push(String(chunk)); return true; };
  try {
    assert.equal(await main(["verify", "--out", join(here, "never-created"), "--plan"]), 0);
    process.env.CODETERM_API_PORT = "7686";
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
  const upgrade = buildPlan({ mode: "upgrade", out: "/o", instance: "curated", port: 7697 });
  assert.ok(upgrade.evidence.includes("/o/n-1-to-n/upgrade.json") && upgrade.evidence.includes("/o/n/restart.json"));
  assert.match(renderPlan(plan), /target: instance=curated port=7697[^]*evidence-contract[.]mjs/);
  assert.throws(() => buildPlan({ mode: "deploy", out: "/o", instance: "x" }), /unknown plan mode/);
  assert.throws(() => planFromEnv({ mode: "all", out: "/o", instance: "", port: "7697" }), /unset/);
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
  for (const key of Object.keys(clean)) if (key.startsWith("CODETERM_")) delete clean[key];
  return spawnSync("bash", [runSh, ...args], { encoding: "utf8", env: { ...clean, ...env }, timeout: 60_000 });
}

test("run.sh --plan exits 0 on a named instance and prints steps and evidence paths", () => {
  const result = runner(["all", "--plan"], { CODETERM_INSTANCE: "curated", CODETERM_API_PORT: "7697" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /PLAN all[^]*J1: [^]*spawn_ready[^]*results[.]json[^]*restart[.]json[^]*setup[.]png/);
});

test("run.sh refuses ports 7685 and 7686 and an unset instance, in plan and live modes", () => {
  for (const mode of [["all", "--plan"], ["all"], ["setup"], ["upgrade"]]) {
    for (const [env, message] of [
      [{ CODETERM_INSTANCE: "curated", CODETERM_API_PORT: "7685" }, /production port 7685/],
      [{ CODETERM_INSTANCE: "curated", CODETERM_API_PORT: "7686" }, /shared DEV port 7686/],
      [{ CODETERM_API_PORT: "7697" }, /CODETERM_INSTANCE is unset/],
      [{}, /CODETERM_INSTANCE is unset/],
    ]) {
      const result = runner(mode, env);
      assert.notEqual(result.status, 0, `${mode.join(" ")} ${JSON.stringify(env)} must be refused`);
      assert.match(result.stderr, message);
    }
  }
});
