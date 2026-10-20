import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { exitCode, validateRun, validateTree } from "./evidence-contract.mjs";
import { fixtureResults, fixtureRestart, writeFixtureRun } from "./fixture-run.mjs";
import { emptyResults, recorder } from "./results.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const scratch = () => mkdtempSync(join(tmpdir(), "cj-contract-"));

function run(mutate = () => {}, { restart = fixtureRestart() } = {}) {
  const dir = scratch();
  const results = fixtureResults();
  mutate(results);
  writeFixtureRun(dir, { results, restart });
  return dir;
}

const errorsOf = (dir) => validateRun(dir).errors.join("\n");

test("a complete run directory passes and exits 0", () => {
  const dir = run();
  const report = validateRun(dir);
  assert.deepEqual(report.errors, []);
  assert.ok(report.pass && report.verdict.pass);
  assert.equal(exitCode([{ dir, ...report }]), 0);
  rmSync(dir, { recursive: true });
});

test("a check without a verdict is rejected", () => {
  const dir = run((results) => { delete results.checks[2].verdict; });
  assert.match(errorsOf(dir), /checks\[2\] J1\/send_reply_own_session: verdict undefined/);
  assert.equal(validateRun(dir).pass, false);
});

test("a check with a bad verdict, a contradiction or a reasonless failure is rejected", () => {
  assert.match(errorsOf(run((results) => { results.checks[1].verdict = "green"; })), /verdict "green"/);
  assert.match(errorsOf(run((results) => { results.checks[1].verdict = "fail"; })), /contradicts pass=true/);
  assert.match(errorsOf(run((results) => { Object.assign(results.checks[1], { pass: false, verdict: "fail" }); })), /fail without a reason/);
});

test("a failed run with complete evidence is valid evidence but exits 3", () => {
  const dir = run((results) => { Object.assign(results.checks[1], { pass: false, verdict: "fail", reason: "deadline" }); });
  const report = validateRun(dir);
  assert.deepEqual(report.errors, []);
  assert.equal(report.verdict.pass, false);
  assert.equal(exitCode([{ dir, ...report }]), 3);
});

test("missing duration, build sha, instance, shared port or journey coverage is rejected", () => {
  assert.match(errorsOf(run((results) => { delete results.checks[0].ms; })), /duration ms missing/);
  assert.match(errorsOf(run((results) => { results.phases[0].build_sha = ""; })), /build_sha missing/);
  assert.match(errorsOf(run((results) => { for (const phase of results.phases) phase.instance = ""; })), /named instance/);
  assert.match(errorsOf(run((results) => { for (const phase of results.phases) phase.port = 7686; })), /shared Domios port/);
  assert.match(errorsOf(run((results) => { results.checks = results.checks.filter((check) => check.journey !== "J3"); })), /no check recorded for J3/);
  assert.match(errorsOf(run((results) => { results.phases = results.phases.filter((phase) => phase.name !== "verify"); })), /phase verify never ran/);
});

test("missing tab, session and listener ids are rejected", () => {
  assert.match(errorsOf(run((results) => { results.state.tabs = []; })), /recorded no tab ids/);
  assert.match(errorsOf(run((results) => { delete results.state.tabs[0].session; })), /recorded no session id/);
  assert.match(errorsOf(run((results) => { delete results.state.before_restart.listener; })), /listener lacks pid/);
  assert.match(errorsOf(run((results) => { results.state.workspace = {}; })), /workspace\.id missing/);
});

test("results.json missing or unparsable is rejected", () => {
  const empty = scratch();
  assert.match(errorsOf(empty), /results\.json: missing/);
  writeFileSync(join(empty, "results.json"), "{not json");
  assert.match(errorsOf(empty), /not valid JSON/);
});

test("results.md missing or drifting from results.json is rejected", () => {
  const dir = run();
  writeFileSync(join(dir, "results.md"), "# Core journeys\n\nVerdict: **PASS**\n");
  assert.match(errorsOf(dir), /results\.md: does not match/);
  rmSync(join(dir, "results.md"));
  assert.match(errorsOf(dir), /results\.md: missing/);
});

test("restart.json missing, failed, malformed or for another target is rejected", () => {
  assert.match(errorsOf(run(() => {}, { restart: null })), /restart\.json: missing/);
  assert.match(errorsOf(run(() => {}, { restart: { ...fixtureRestart(), pass: false, reason: "restart_not_observed" } })), /pass is false \(restart_not_observed\)/);
  assert.match(errorsOf(run(() => {}, { restart: { ...fixtureRestart(), after: { pid: 100, started_us: 1000 } } })), /do not show a restart/);
  assert.match(errorsOf(run(() => {}, { restart: { ...fixtureRestart(), port: 7000 } })), /differs from results\.json/);
});

test("screenshots must be recorded for setup and verify and be real PNG files", () => {
  assert.match(errorsOf(run((results) => { results.state.screenshots = []; })), /none recorded for phase setup/);
  const missing = run();
  rmSync(join(missing, "screenshots", "verify.png"));
  assert.match(errorsOf(missing), /verify file screenshots\/verify\.png is missing/);
  const bad = run();
  writeFileSync(join(bad, "screenshots", "setup.png"), "not a png at all");
  assert.match(errorsOf(bad), /setup\.png is not a PNG/);
});

test("an upgrade run directory validates both cycles, using upgrade.json for the first", () => {
  const dir = scratch();
  writeFixtureRun(join(dir, "n"));
  writeFixtureRun(join(dir, "n-1-to-n"), { restart: null });
  writeFileSync(join(dir, "n-1-to-n", "upgrade.json"), JSON.stringify(fixtureRestart()));
  assert.deepEqual(validateTree(dir).map((report) => report.pass), [true, true]);
  rmSync(join(dir, "n", "restart.json"));
  assert.deepEqual(validateTree(dir).map((report) => report.pass), [true, false]);
});

test("the CLI exits 0 on the complete fixture and 1 on a broken one", () => {
  const cli = join(here, "evidence-contract.mjs");
  assert.equal(spawnSync(process.execPath, [cli, run()], { encoding: "utf8" }).status, 0);
  const result = spawnSync(process.execPath, [cli, run((results) => { delete results.checks[0].verdict; })], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.match(result.stdout, /EVIDENCE INVALID/);
  assert.equal(spawnSync(process.execPath, [cli], { encoding: "utf8" }).status, 2);
});

test("the real recorder stamps every entry with a verdict", async () => {
  const results = emptyResults();
  const check = recorder(scratch(), results, { log: () => {} });
  await check("J1", "spawn_ready", "claude", async () => "ok");
  await check("J1", "stop_long_turn", "claude", async () => { throw new Error("boom"); });
  assert.deepEqual(results.checks.map((entry) => entry.verdict), ["pass", "fail"]);
});
