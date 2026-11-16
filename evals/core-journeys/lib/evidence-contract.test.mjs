// WHAT: Persisted full-journey results and model-policy evidence.
// WHY: A passing turn cannot establish prelaunch model-policy compliance.
// ORACLE: The evidence validator rejects missing or substituted selections and identities.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { exitCode, validateRun, validateTree } from "./evidence-contract.mjs";
import { fixtureResults, fixtureRestart, writeFixtureRun } from "./fixture-run.mjs";
import { emptyResults, recorder } from "./results.mjs";
import { modelEvidenceErrors } from "./model-evidence.mjs";
import { profileSelection } from "./profile-policy.mjs";
import { emptyFlags, loadAreas, resolveSelection } from "./selection.mjs";

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

function withProfileEvidence() {
  const { state } = fixtureResults(), policy = state.model_policy;
  const selected = "gemini-cli";
  const entry = profileSelection({ id: "acp", agent_runtime: { launchProfiles: { [selected]: {} } } }, selected,
    [{ id: selected, install: "detected", auth: "unknown", configOptions: [] }]);
  policy.roster.push("acp"); policy.selections.acp = selected; policy.catalogues.acp = entry;
  policy.verify_catalogues = { acp: structuredClone(entry) };
  for (const configuration of Object.values(policy.configurations)) configuration.defaults = { ...configuration.defaults, acp: null };
  const identity = { provider: "acp", selected, launch_profile: selected, requested: null, resolved: null, actual: null, session: "acp-session" };
  policy.identities.acpTab = identity; policy.restored_identities.acpTab = { ...identity };
  policy.launches.push({ provider: "acp", path: "J1-spawn", model: null, selection: selected, launch_profile: selected, catalogue: entry, checked_at: "2026-01-01T00:00:00Z" });
  state.tabs.push({ id: "acpTab", provider: "acp", journey: "J1", session: identity.session });
  state.before_restart.tabs.push({ id: "acpTab", agent_type: "acp", session: identity.session, owned: true, model_identity: identity });
  return state;
}

test("evidence accepts profile-default identity without claiming an actual model, including restart", () => {
  assert.deepEqual(modelEvidenceErrors(withProfileEvidence()), []);
});

test("evidence refuses profile substitution, missing readiness and invented default-model evidence", () => {
  for (const mutate of [
    state => { state.model_policy.launches.at(-1).launch_profile = "other"; },
    state => { state.model_policy.catalogues.acp.profile.install = "not_installed"; },
    state => { state.model_policy.restored_identities.acpTab.launch_profile = "other"; },
    state => { state.model_policy.configurations.verify.defaults.acp = "gemini-cli"; },
  ]) {
    const state = withProfileEvidence(); mutate(state);
    assert.ok(modelEvidenceErrors(state).length > 0);
  }
});

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

test("missing policy, narrowed roster and substituted prelaunch evidence are invalid", () => {
  assert.match(errorsOf(run((results) => { delete results.state.model_policy; })), /persisted policy absent/);
  assert.match(errorsOf(run((results) => { results.state.model_policy.roster = ["claude"]; })), /required provider roster narrowed/);
  assert.match(errorsOf(run((results) => { results.state.model_policy.launches[0].model = "substitution"; })), /model substituted/);
  assert.match(errorsOf(run((results) => { results.state.model_policy.configurations.setup.general_model = "substitution"; })), /General selection absent or substituted/);
  assert.match(errorsOf(run((results) => { results.state.model_policy.catalogues.claude.refresh.refreshed = false; })), /refresh unavailable/);
  assert.match(errorsOf(run((results) => { results.state.tabs[0].control_fixture = true; })), /control fixture row absent or ambiguous/);
  assert.match(errorsOf(run((results) => { results.state.j3.fixture_provider = "claude"; })), /control fixture provider differs/);
  assert.match(errorsOf(run((results) => { results.state.tabs[0].shell = true; delete results.state.model_policy.identities.tab1; })), /selected identity absent/);
});

test("requested, resolved and actual model identities and restart proof must all persist", () => {
  for (const field of ["requested", "resolved", "actual"]) {
    assert.match(errorsOf(run((results) => { results.state.model_policy.identities.tab1[field] = null; })), /unknown/);
  }
  assert.match(errorsOf(run((results) => { delete results.state.model_policy.restored_identities.tab1; })), /selected identity absent/);
  assert.match(errorsOf(run((results) => { results.state.model_policy.restored_identities.tab1.session = "foreign"; })), /restored policy session changed/);
});


const select = (extra) => resolveSelection(loadAreas(), { ...emptyFlags(), ...extra, exclude_providers: extra.exclude_providers ?? [] });

function withoutControlFixture(results) {
  const control = results.state.j3.orchestrator;
  results.state.tabs = results.state.tabs.filter((tab) => tab.id !== control);
  if (results.state.before_restart) results.state.before_restart.tabs = results.state.before_restart.tabs.filter((tab) => tab.id !== control);
  delete results.state.j3;
}

function j4Only(results) {
  results.selection = select({ journeys: "J4" });
  results.checks = results.checks.filter((check) => !["J1", "J2", "J3"].includes(check.journey));
  results.checks.splice(1, 0, ...["restore_fixture_ready", "restore_fixture_reply"].map((name) => ({ ...results.checks[0], journey: "J4", name, subject: "claude" })));
  withoutControlFixture(results);
  for (const tab of results.state.tabs) tab.journey = "J4";
  results.state.model_policy.launches = [{ ...results.state.model_policy.launches[0], path: "J4-spawn" }];
}

function j1Only(results) {
  results.selection = select({ journeys: "J1" });
  results.phases = results.phases.filter((phase) => phase.name !== "verify");
  results.checks = results.checks.filter((check) => !["J2", "J3", "J4"].includes(check.journey) && check.subject !== "verify");
  withoutControlFixture(results);
  delete results.state.before_restart;
  results.state.screenshots = results.state.screenshots.filter((shot) => shot.phase === "setup");
  results.state.model_policy.launches = results.state.model_policy.launches.filter((launch) => launch.path === "J1-spawn");
}

test("a J4-only run with its own restore fixtures is complete evidence; checks of unselected journeys are not", () => {
  assert.deepEqual(errorsOf(run(j4Only)), "");
  assert.match(errorsOf(run((results) => { j4Only(results); results.checks.push({ ...results.checks[0], journey: "J1", name: "spawn_ready" }); })),
    /J1 is recorded as not run or waived but has checks/);
  assert.match(errorsOf(run((results) => { j4Only(results); results.state.tabs[0].journey = "J1"; })), /claude J4 roster coverage absent/);
});

test("a run without J4 needs no restart or verify phase and refuses one that ran", () => {
  assert.deepEqual(errorsOf(run(j1Only, { restart: null })), "");
  assert.match(errorsOf(run((results) => { j1Only(results); results.phases.push({ ...results.phases[0], name: "verify" }); }, { restart: null })),
    /phase verify ran although J4 is not selected/);
});

test("an explicitly waived provider exclusion is valid and rendered verbatim; a silent or tampered one is not", () => {
  const waiver = "owner: opencode | provider outage";
  const exclude = (results) => {
    results.selection = select({ exclude_providers: ["opencode"], waiver });
    const policy = results.state.model_policy;
    policy.roster = ["claude", "codex"];
    for (const map of [policy.selections, policy.catalogues]) delete map.opencode;
    for (const map of [policy.identities, policy.restored_identities]) delete map.tab3;
    results.state.tabs = results.state.tabs.filter((tab) => tab.id !== "tab3");
    results.state.before_restart.tabs = results.state.before_restart.tabs.filter((tab) => tab.id !== "tab3");
  };
  const dir = run(exclude);
  assert.deepEqual(errorsOf(dir), "");
  assert.ok(readFileSync(join(dir, "results.md"), "utf8").includes(`--waiver ${waiver}\n`));
  assert.match(errorsOf(run((results) => { exclude(results); results.selection = select({}); })), /required provider roster narrowed/);
  assert.match(errorsOf(run((results) => { exclude(results); results.selection.excluded_providers[0].waiver = "edited"; })), /selection differs/);
  assert.match(errorsOf(run((results) => { results.selection.journeys.J2 = { status: "not_run", reasons: ["area untouched"] }; })), /selection differs/);
});

function qualifyCachedVerify(results) {
  const policy = results.state.model_policy, provider = "opencode";
  const build = results.phases.find(phase => phase.name === "setup").build_sha;
  policy.setup_live = { [provider]: { run: results.state.run, build_sha: build,
    catalogue: structuredClone(policy.catalogues[provider]) } };
  const entry = structuredClone(policy.catalogues[provider]);
  entry.refresh.refreshed = false;
  entry.refresh.reason = "the provider reported no live model list";
  entry.qualification = { verified_live_in_setup: true, post_restart_refresh_in_flight: true,
    model_present_in_cache: true, model_id: policy.selections[provider], run: results.state.run, build_sha: build };
  policy.catalogues[provider] = entry;
  policy.verify_catalogues = { [provider]: structuredClone(entry) };
}

test("validator accepts explicit verify cache qualification backed by same-run live setup", () => {
  const dir = run(qualifyCachedVerify);
  try { assert.deepEqual(validateRun(dir).errors, []); } finally { rmSync(dir, { recursive: true }); }
});

test("validator rejects cached verification without live proof or with a different actual phase build/model", () => {
  for (const mutate of [
    results => { delete results.state.model_policy.setup_live.opencode; },
    results => { results.state.model_policy.setup_live.opencode.catalogue.refresh.refreshed = false; },
    results => { results.phases.find(phase => phase.name === "verify").build_sha = "different-build"; },
    results => { results.state.model_policy.verify_catalogues.opencode.model.id = "substituted"; },
    results => { results.state.model_policy.catalogues.opencode.qualification.verified_live_in_setup = false; },
  ]) {
    const dir = run(results => { qualifyCachedVerify(results); mutate(results); });
    try { assert.equal(validateRun(dir).pass, false); } finally { rmSync(dir, { recursive: true }); }
  }
});
