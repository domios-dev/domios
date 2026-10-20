import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { emptyResults, saveResults } from "./results.mjs";

const PNG = Buffer.from("89504e470d0a1a0a0000000d4948445200000001000000010806000000" + "1f15c4890000000d49444154789c6360000002000001e221bc330000000049454e44ae426082", "hex");
const check = (journey, name, subject, extra = {}) => ({
  journey, name, subject, pass: true, verdict: "pass", started_at: "2026-01-01T00:00:00.000Z", ms: 1200, summary: "ok", ...extra,
});

export function fixtureResults() {
  const results = emptyResults(new Date("2026-01-01T00:00:00Z"));
  const target = { instance: "fixture", port: 7697, build_sha: "0123456789abcdef" };
  results.phases = ["setup", "verify", "cleanup"].map((name) => ({ name, ...target, started_at: "2026-01-01T00:00:00.000Z", wall_ms: 5000 }));
  results.checks = [
    check("SETUP", "workspace_provisioned", "run1"),
    check("J1", "spawn_ready", "claude"),
    check("J1", "send_reply_own_session", "claude"),
    check("J2", "api_no_context", "claude"),
    check("J3", "dispatch_chat_backend_worker", "codex"),
    check("J4", "pre_restart_snapshot", "run1"),
    check("EVIDENCE", "window_screenshot", "setup"),
    check("J4", "restart_observed", "fixture:7697"),
    check("J4", "restored_tab_ready", "tab1 claude"),
    check("EVIDENCE", "window_screenshot", "verify"),
    check("CLEAN", "workspace_closed", "ws1"),
  ];
  results.state = {
    run: "run1",
    workspace: { id: "ws1", cwd: "/tmp/ws1" },
    tabs: [{ id: "tab1", provider: "claude", journey: "J1", reply: true, session: "session1" }],
    teams: [], plans: [],
    before_restart: {
      listener: { pid: 100, started_us: 1000 }, build_sha: target.build_sha,
      tabs: [{ id: "tab1", agent_type: "claude", session: "session1", owned: true }],
    },
    screenshots: [{ phase: "setup", file: "screenshots/setup.png", bytes: PNG.length }, { phase: "verify", file: "screenshots/verify.png", bytes: PNG.length }],
  };
  return results;
}

export function fixtureRestart() {
  return { instance: "fixture", port: 7697, label: "restart", pass: true, phase: "verified", before: { pid: 100, started_us: 1000 }, after: { pid: 200, started_us: 2000 } };
}

export function writeFixtureRun(dir, { results = fixtureResults(), restart = fixtureRestart() } = {}) {
  mkdirSync(join(dir, "screenshots"), { recursive: true });
  saveResults(dir, results);
  if (restart) writeFileSync(join(dir, "restart.json"), `${JSON.stringify(restart, null, 2)}\n`);
  for (const phase of ["setup", "verify"]) writeFileSync(join(dir, "screenshots", `${phase}.png`), PNG);
  return dir;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2]) {
    console.error("Usage: node evals/core-journeys/lib/fixture-run.mjs <empty-dir>");
    process.exitCode = 2;
  } else {
    writeFixtureRun(resolve(process.argv[2]));
    console.log(`fixture run written to ${resolve(process.argv[2])}`);
  }
}
