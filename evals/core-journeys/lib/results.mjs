import { existsSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const JOURNEYS = {
  SETUP: "Disposable workspace provisioned",
  J1: "Spawn, send, reply and stop per installed provider",
  J2: "Team start from the Start-a-session dialog API and UI context",
  J3: "Dispatch, ACK, report, bounce and accept for a chat_backend Worker",
  J4: "DEV restart: every restored tab ready, sends answered, attempt bindings intact",
  CLEAN: "Owned plans, Teams, tabs and workspace removed",
  EVIDENCE: "Window screenshots captured for the run",
  HARNESS: "Harness error outside a journey check",
};

export function emptyResults(now = new Date()) {
  return { schema: 1, started_at: now.toISOString(), phases: [], checks: [], state: {} };
}

export function loadResults(out) {
  const path = join(out, "results.json");
  return existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : emptyResults();
}

export function saveResults(out, results) {
  const path = join(out, "results.json");
  writeFileSync(`${path}.tmp`, `${JSON.stringify(results, null, 2)}\n`);
  renameSync(`${path}.tmp`, path);
  writeFileSync(join(out, "results.md"), renderMarkdown(results));
}

export function journeySummary(checks) {
  const summary = {};
  for (const check of checks) {
    const entry = summary[check.journey] ??= { pass: 0, fail: 0, ms: 0 };
    entry[check.pass ? "pass" : "fail"] += 1;
    entry.ms += check.ms ?? 0;
  }
  return summary;
}

export function verdict(results) {
  const failed = results.checks.filter((check) => !check.pass);
  if (!results.checks.length) return { pass: false, reason: "no checks executed" };
  return failed.length ? { pass: false, reason: `${failed.length} check(s) failed` } : { pass: true, reason: "all checks passed" };
}

const cell = (value) => String(value ?? "—").replaceAll("|", "\\|").replaceAll("\n", " ");
const seconds = (ms) => (ms == null ? "—" : `${(ms / 1000).toFixed(1)}s`);

const STATUS_TEXT = { run: "run", not_run: "not run", waived: "waived (accepted risk)" };
const SHOWN_REASONS = 5;

function renderSelection(selection) {
  const { flags, selector } = selection;
  const argv = [
    ...(flags.journeys === null ? [] : [`--journeys ${flags.journeys}`]),
    ...(flags.selection === null ? [] : [`--selection ${flags.selection}`]),
    ...flags.exclude_providers.map((id) => `--exclude-provider ${id}`),
    ...(flags.waiver === null ? [] : [`--waiver ${flags.waiver}`]),
  ];
  const lines = [`Selection: \`${selection.source}\`${selector ? ` from diff ${selector.base}..${selector.head} (${selector.changed_paths.length} changed paths)` : ""}.`, ""];
  lines.push("Selection flags, verbatim:", "", "```text", ...(argv.length ? argv : ["(none: full suite)"]), "```", "");
  lines.push("| Journey | Status | Reasons |", "| --- | --- | --- |");
  for (const [id, entry] of Object.entries(selection.journeys)) {
    const reasons = entry.reasons.slice(0, SHOWN_REASONS).join("; ") + (entry.reasons.length > SHOWN_REASONS ? `; and ${entry.reasons.length - SHOWN_REASONS} more in results.json` : "");
    const status = entry.status === "not_run" ? `not run: ${entry.reasons.join("; ")}` : STATUS_TEXT[entry.status];
    lines.push(`| ${cell(id)} | ${cell(status)} | ${cell(entry.status === "waived" ? `${reasons}; waived under --waiver` : reasons)} |`);
  }
  for (const { id } of selection.excluded_providers) lines.push("", `Provider \`${id}\` excluded from the roster (accepted risk) under --waiver.`);
  return lines;
}

export function renderMarkdown(results) {
  const outcome = verdict(results);
  const lines = ["# Core journeys", "", `Started: ${results.started_at}`, ""];
  for (const phase of results.phases) {
    lines.push(`- Phase \`${phase.name}\`: build ${cell(phase.build_sha)} on ${cell(phase.instance || "default")}:${cell(phase.port)}, wall ${seconds(phase.wall_ms)}`);
  }
  if (results.selection) lines.push("", ...renderSelection(results.selection));
  const wall = results.phases.reduce((sum, phase) => sum + (phase.wall_ms ?? 0), 0);
  lines.push("", `Verdict: **${outcome.pass ? "PASS" : "FAIL"}** (${outcome.reason}); total phase wall time ${seconds(wall)}.`, "");
  lines.push("| Journey | Pass | Fail | Time |", "| --- | --- | --- | --- |");
  for (const [journey, entry] of Object.entries(journeySummary(results.checks))) {
    lines.push(`| ${journey} ${cell(JOURNEYS[journey])} | ${entry.pass} | ${entry.fail} | ${seconds(entry.ms)} |`);
  }
  lines.push("", "| Journey | Check | Subject | Verdict | Time | Reason / evidence |", "| --- | --- | --- | --- | --- | --- |");
  for (const check of results.checks) {
    lines.push(`| ${[check.journey, check.name, check.subject, check.pass ? "PASS" : "FAIL", seconds(check.ms),
      check.reason ?? check.summary ?? ""].map(cell).join(" | ")} |`);
  }
  return `${lines.join("\n")}\n`;
}

export function recorder(out, results, { log = console.log } = {}) {
  return async function check(journey, name, subject, body) {
    const started = performance.now();
    const entry = { journey, name, subject, pass: false, started_at: new Date().toISOString() };
    try {
      const value = await body();
      entry.pass = true;
      entry.summary = typeof value === "string" ? value : value?.summary;
      entry.evidence = value?.evidence ?? value;
      return value;
    } catch (error) {
      entry.reason = error?.reason ?? error?.message ?? String(error);
      entry.evidence = error?.evidence ?? null;
      return undefined;
    } finally {
      entry.verdict = entry.pass ? "pass" : "fail";
      entry.ms = Math.round(performance.now() - started);
      results.checks.push(entry);
      saveResults(out, results);
      log(`${entry.pass ? "PASS" : "FAIL"} ${journey} ${name} ${subject ?? ""} ${entry.pass ? "" : entry.reason}`.trimEnd());
    }
  };
}
