import { existsSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { restartObserved } from "../../restart-endurance/restart.mjs";
import { JOURNEYS, renderMarkdown, verdict } from "./results.mjs";

export const VERDICTS = ["pass", "fail", "skip"];
export const PHASES = ["setup", "verify", "cleanup"];
export const REQUIRED_JOURNEYS = ["SETUP", "J1", "J2", "J3", "J4"];
const SHARED_PORTS = [7685, 7686];
const PNG_MAGIC = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

const isText = (value) => typeof value === "string" && value.trim() !== "";
const isCount = (value) => Number.isFinite(value) && value >= 0;
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

function readJson(dir, name, errors) {
  const path = join(dir, name);
  if (!existsSync(path)) { errors.push(`${name}: missing`); return null; }
  try { return JSON.parse(readFileSync(path, "utf8")); } catch (error) { errors.push(`${name}: not valid JSON (${error.message})`); return null; }
}

function checkPhases(results, expected, errors) {
  if (!Array.isArray(results.phases) || !results.phases.length) { errors.push("results.json: phases is empty"); return; }
  const names = results.phases.map((phase) => phase?.name);
  for (const name of expected) if (!names.includes(name)) errors.push(`results.json: phase ${name} never ran`);
  results.phases.forEach((phase, index) => {
    const at = `results.json: phases[${index}]`;
    if (!PHASES.includes(phase?.name)) errors.push(`${at}.name ${JSON.stringify(phase?.name)} is not one of ${PHASES.join("|")}`);
    if (!isText(phase?.instance)) errors.push(`${at}.instance missing: the run did not record a named instance`);
    if (!Number.isInteger(phase?.port)) errors.push(`${at}.port missing`);
    else if (SHARED_PORTS.includes(phase.port)) errors.push(`${at}.port ${phase.port} is a shared Domios port`);
    if (!isText(phase?.build_sha)) errors.push(`${at}.build_sha missing`);
    if (!isCount(phase?.wall_ms)) errors.push(`${at}.wall_ms missing`);
  });
  const targets = new Set(results.phases.map((phase) => `${phase?.instance}:${phase?.port}`));
  if (targets.size > 1) errors.push(`results.json: phases disagree on instance:port (${[...targets].join(", ")})`);
}

function checkChecks(results, journeys, errors) {
  if (!Array.isArray(results.checks) || !results.checks.length) { errors.push("results.json: checks is empty"); return; }
  results.checks.forEach((check, index) => {
    const at = `results.json: checks[${index}] ${check?.journey ?? "?"}/${check?.name ?? "?"}`;
    if (!Object.hasOwn(JOURNEYS, check?.journey)) errors.push(`${at}: unknown journey id`);
    if (!isText(check?.name)) errors.push(`${at}: name missing`);
    if (!VERDICTS.includes(check?.verdict)) errors.push(`${at}: verdict ${JSON.stringify(check?.verdict)} is not one of ${VERDICTS.join("|")}`);
    else if ((check.verdict === "fail") === (check.pass === true)) errors.push(`${at}: verdict ${check.verdict} contradicts pass=${check.pass}`);
    else if (check.verdict !== "pass" && !isText(check.reason)) errors.push(`${at}: ${check.verdict} without a reason`);
    if (!isCount(check?.ms)) errors.push(`${at}: duration ms missing`);
    if (!isText(check?.started_at)) errors.push(`${at}: started_at missing`);
  });
  for (const journey of journeys) {
    if (!results.checks.some((check) => check?.journey === journey)) errors.push(`results.json: no check recorded for ${journey}`);
  }
}

function checkIds(results, expected, errors) {
  const state = results.state;
  if (!isObject(state)) { errors.push("results.json: state missing"); return; }
  if (!isText(state.run)) errors.push("results.json: state.run (run id) missing");
  if (!isText(state.workspace?.id)) errors.push("results.json: state.workspace.id missing");
  const tabs = Array.isArray(state.tabs) ? state.tabs : [];
  if (!tabs.length) errors.push("results.json: state.tabs recorded no tab ids");
  tabs.forEach((tab, index) => { if (!isText(tab?.id)) errors.push(`results.json: state.tabs[${index}].id missing`); });
  const replied = new Set(results.checks.filter((check) => check?.journey === "J1" && check.name === "send_reply_own_session" && check.verdict === "pass").map((check) => check.subject));
  for (const tab of tabs.filter((entry) => entry?.journey === "J1" && replied.has(entry.provider))) {
    if (!isText(tab.session)) errors.push(`results.json: J1 tab ${tab.id} answered but recorded no session id`);
  }
  if (!expected.includes("verify")) return;
  const before = state.before_restart;
  if (!isObject(before)) { errors.push("results.json: state.before_restart missing"); return; }
  if (!Number.isInteger(before.listener?.pid) || !Number.isSafeInteger(before.listener?.started_us)) errors.push("results.json: state.before_restart.listener lacks pid/started_us");
  if (!isText(before.build_sha)) errors.push("results.json: state.before_restart.build_sha missing");
  if (!Array.isArray(before.tabs) || !before.tabs.length) errors.push("results.json: state.before_restart.tabs recorded no tab ids");
  else before.tabs.forEach((tab, index) => { if (!isText(tab?.id)) errors.push(`results.json: state.before_restart.tabs[${index}].id missing`); });
}

function checkScreenshots(dir, results, expected, errors) {
  const shots = Array.isArray(results.state?.screenshots) ? results.state.screenshots : [];
  for (const phase of expected.filter((name) => name !== "cleanup")) {
    const shot = shots.find((entry) => entry?.phase === phase);
    if (!shot) { errors.push(`screenshots: none recorded for phase ${phase}`); continue; }
    const path = join(dir, String(shot.file ?? ""));
    if (!isText(shot.file) || !existsSync(path)) { errors.push(`screenshots: ${phase} file ${shot.file} is missing`); continue; }
    const bytes = readFileSync(path);
    if (bytes.length <= PNG_MAGIC.length || !bytes.subarray(0, PNG_MAGIC.length).equals(PNG_MAGIC)) errors.push(`screenshots: ${shot.file} is not a PNG`);
  }
}

function checkMarkdown(dir, results, errors) {
  const path = join(dir, "results.md");
  if (!existsSync(path)) { errors.push("results.md: missing"); return; }
  const text = readFileSync(path, "utf8").replace(/\r\n/g, "\n");
  if (text !== renderMarkdown(results)) errors.push("results.md: does not match the rendering of results.json");
}

function checkRestart(dir, results, file, errors) {
  const restart = readJson(dir, file, errors);
  if (!restart) return;
  const at = `${file}`;
  if (restart.pass !== true) errors.push(`${at}: pass is ${JSON.stringify(restart.pass)}${restart.reason ? ` (${restart.reason})` : ""}`);
  if (restart.phase !== "verified") errors.push(`${at}: phase is ${JSON.stringify(restart.phase)}, expected verified`);
  if (!restartObserved(restart.before, restart.after)) errors.push(`${at}: before/after listener identities do not show a restart`);
  const phase = results?.phases?.[0];
  if (phase && (restart.instance !== phase.instance || restart.port !== phase.port)) {
    errors.push(`${at}: instance:port ${restart.instance}:${restart.port} differs from results.json ${phase.instance}:${phase.port}`);
  }
}

export function validateRun(dir, { phases = PHASES, journeys = REQUIRED_JOURNEYS, restartFile = "restart.json" } = {}) {
  const errors = [];
  if (!existsSync(dir) || !statSync(dir).isDirectory()) return { pass: false, errors: [`${dir}: not a directory`], verdict: null };
  const results = readJson(dir, "results.json", errors);
  if (results) {
    if (results.schema !== 1) errors.push(`results.json: schema ${JSON.stringify(results.schema)}, expected 1`);
    if (!isText(results.started_at)) errors.push("results.json: started_at missing");
    checkPhases(results, phases, errors);
    checkChecks(results, journeys, errors);
    checkIds(results, phases, errors);
    checkScreenshots(dir, results, phases, errors);
    checkMarkdown(dir, results, errors);
  }
  if (phases.includes("verify")) checkRestart(dir, results, restartFile, errors);
  const outcome = results && Array.isArray(results.checks) ? verdict(results) : null;
  return { pass: errors.length === 0, errors, verdict: outcome };
}

export function validateTree(dir) {
  if (existsSync(join(dir, "results.json")) || !existsSync(join(dir, "n-1-to-n"))) return [{ dir, ...validateRun(dir) }];
  return [
    { dir: join(dir, "n-1-to-n"), ...validateRun(join(dir, "n-1-to-n"), { restartFile: "upgrade.json" }) },
    { dir: join(dir, "n"), ...validateRun(join(dir, "n")) },
  ];
}

export function renderReport(reports) {
  const lines = [];
  for (const report of reports) {
    lines.push(`${report.pass ? "EVIDENCE OK" : "EVIDENCE INVALID"} ${report.dir}`);
    for (const error of report.errors) lines.push(`  - ${error}`);
    if (report.verdict) lines.push(`  run verdict: ${report.verdict.pass ? "PASS" : "FAIL"} (${report.verdict.reason})`);
  }
  return `${lines.join("\n")}\n`;
}

export function exitCode(reports) {
  if (reports.some((report) => !report.pass)) return 1;
  return reports.every((report) => report.verdict?.pass) ? 0 : 3;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const dir = process.argv[2];
  if (!dir) {
    console.error("Usage: node evals/core-journeys/lib/evidence-contract.mjs <run-dir>\nExit 0: evidence complete and the run passed; 1: evidence missing or malformed; 3: evidence complete but the run failed.");
    process.exitCode = 2;
  } else {
    const reports = validateTree(resolve(dir));
    process.stdout.write(renderReport(reports));
    process.exitCode = exitCode(reports);
  }
}
