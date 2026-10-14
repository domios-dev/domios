import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { assertIsolated } from "./env.mjs";
import { JOURNEYS } from "./results.mjs";
import { screenshotPath } from "./screenshots.mjs";

const J1 = ["spawn_ready", "send_reply_own_session", "stop_long_turn", "long_history_turn (runtime providers)"];
const J2 = ["api_no_context", "ui_manager_context", "ui_worker_context"];
const J3 = ["fixture_orchestrator_plan", "dispatch_chat_backend_worker", "worker_ack", "worker_report_in_review", "bounce_then_report"];
const J4_AFTER = ["restart_observed", "restored_tab_ready", "restored_send_reply", "restored_worker_attempt_binding",
  "fixture_orchestrator_rebind", "restored_bounce_then_report", "review_accept_done"];

export const PHASE_STEPS = {
  setup: [["SETUP", ["workspace_provisioned"]], ["J1", J1], ["J2", J2], ["J3", J3], ["J4", ["pre_restart_snapshot"]], ["EVIDENCE", ["window_screenshot setup"]]],
  verify: [["J4", J4_AFTER], ["EVIDENCE", ["window_screenshot verify"]]],
  cleanup: [["CLEAN", ["plan_deleted", "team_closed", "workspace_closed"]]],
};

const RESTART_STEP = "bench-owned restart: build.sh kill, then build.sh dev-prebuilt --wait --wait-build; restart.json records the listener before and after";

export function cycleEvidence(dir, restartFile = "restart.json") {
  return [`${dir}/results.json`, `${dir}/results.md`, `${dir}/${restartFile}`, `${dir}/${screenshotPath("setup")}`, `${dir}/${screenshotPath("verify")}`];
}

function phasePlan(name) {
  return { phase: name, steps: PHASE_STEPS[name].flatMap(([journey, checks]) => checks.map((check) => ({ journey, check, title: JOURNEYS[journey] }))) };
}

export function buildPlan({ mode, out, instance, port, from = null }) {
  const sequence = { setup: ["setup"], verify: ["verify"], cleanup: ["cleanup"], all: ["setup", "restart", "verify", "cleanup"],
    upgrade: ["setup", "restart", "verify", "cleanup", "setup", "restart", "verify", "cleanup"] }[mode];
  if (!sequence) throw new Error(`unknown plan mode ${mode}`);
  const stages = sequence.map((name) => (name === "restart" ? { phase: "restart", steps: [{ journey: "J4", check: RESTART_STEP }] } : phasePlan(name)));
  const evidence = mode === "upgrade"
    ? [...cycleEvidence(`${out}/n-1-to-n`, "upgrade.json"), ...cycleEvidence(`${out}/n`)]
    : mode === "all" ? cycleEvidence(out) : [`${out}/results.json`, `${out}/results.md`, ...(mode === "cleanup" ? [] : [`${out}/${screenshotPath(mode)}`])];
  return { mode, out, target: { instance, port: port ?? null }, from, stages, evidence };
}

export function renderPlan(plan) {
  const port = plan.target.port ?? "resolved from the instance by identify";
  const lines = [`PLAN ${plan.mode} (dry: no instance contacted, nothing written)`, `target: instance=${plan.target.instance} port=${port}`];
  if (plan.from) lines.push(`N-1 checkout: ${plan.from}`);
  plan.stages.forEach((stage, index) => {
    lines.push(`stage ${index + 1}: ${stage.phase}`);
    let last = null;
    for (const step of stage.steps) {
      if (step.title && step.journey !== last) lines.push(`  ${step.journey}: ${step.title}`);
      last = step.journey;
      lines.push(`    ${step.check}`);
    }
  });
  lines.push("evidence:", ...plan.evidence.map((path) => `  ${path}`));
  lines.push(`check: node evals/core-journeys/lib/evidence-contract.mjs ${plan.out}`);
  return `${lines.join("\n")}\n`;
}

export function planFromEnv({ mode, out, instance, port, from }) {
  const parsed = port === undefined || port === "" ? null : Number(port);
  assertIsolated({ instance, port: parsed });
  return buildPlan({ mode, out: resolve(out), instance: instance.trim(), port: parsed, from });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, ...rest] = process.argv.slice(2);
  const flag = (name) => (rest.includes(name) ? rest[rest.indexOf(name) + 1] : undefined);
  try {
    process.stdout.write(renderPlan(planFromEnv({
      mode, out: flag("--out"), instance: flag("--instance") ?? "", port: flag("--port"), from: flag("--from") ?? null,
    })));
  } catch (error) {
    console.error(`FAIL ${error.message}`);
    process.exitCode = 7;
  }
}
