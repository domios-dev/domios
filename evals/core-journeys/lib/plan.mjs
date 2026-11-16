import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { assertIsolated } from "./env.mjs";
import { JOURNEYS } from "./results.mjs";
import { screenshotPath } from "./screenshots.mjs";
import { loadAreas, runs, scanSelectionFlags, selectionFromFlags } from "./selection.mjs";

const J1 = ["spawn_ready", "send_reply_own_session", "stop_long_turn", "long_history_turn (runtime providers)"];
const J2 = ["api_no_context", "ui_manager_context", "ui_worker_context"];
const J3 = ["fixture_orchestrator_plan", "dispatch_chat_backend_worker", "worker_ack", "worker_report_in_review", "bounce_then_report"];
const J4_OWN_SETUP = ["restore_fixture_ready", "restore_fixture_reply"];
const J4_AFTER = ["restart_observed", "restored_tab_ready", "restored_send_reply"];

// Each step group is [journey, checks, when(selected)]; selected is the Set of journeys that run.
const always = () => true;
const has = (id) => (selected) => selected.has(id);
export const PHASE_STEPS = {
  setup: [["SETUP", ["agent_access_chosen", "workspace_provisioned"], always], ["J1", J1, has("J1")],
    ["J4", J4_OWN_SETUP, (selected) => selected.has("J4") && !selected.has("J1")], ["J2", J2, has("J2")], ["J3", J3, has("J3")],
    ["J3", ["review_accept_done"], (selected) => selected.has("J3") && !selected.has("J4")],
    ["J4", ["pre_restart_snapshot"], has("J4")], ["EVIDENCE", ["window_screenshot setup"], always]],
  verify: [["J4", J4_AFTER, always], ["J4", ["restored_worker_attempt_binding", "fixture_orchestrator_rebind"], has("J3")],
    ["J3", ["restored_bounce_then_report", "review_accept_done"], has("J3")], ["EVIDENCE", ["window_screenshot verify"], always]],
  cleanup: [["CLEAN", ["plan_deleted", "team_closed", "workspace_closed"], always]],
};

const RESTART_STEP = "bench-owned restart: build.sh kill, then build.sh dev-prebuilt --wait --wait-build; restart.json records the listener before and after";

export function cycleEvidence(dir, restartFile = "restart.json") {
  return [`${dir}/results.json`, `${dir}/results.md`, `${dir}/${restartFile}`, `${dir}/${screenshotPath("setup")}`, `${dir}/${screenshotPath("verify")}`];
}

function phasePlan(name, selected) {
  return { phase: name, steps: PHASE_STEPS[name].filter(([, , when]) => when(selected))
    .flatMap(([journey, checks]) => checks.map((check) => ({ journey, check, title: JOURNEYS[journey] }))) };
}

export function buildPlan({ mode, out, instance, port, selection = null }) {
  const selected = new Set(selection ? Object.keys(selection.journeys).filter((id) => runs(selection, id)) : ["J1", "J2", "J3", "J4"]);
  const restarts = selected.has("J4");
  const cycle = restarts ? ["setup", "restart", "verify", "cleanup"] : ["setup", "cleanup"];
  const sequence = { setup: ["setup"], verify: ["verify"], cleanup: ["cleanup"], all: cycle }[mode];
  if (!sequence) throw new Error(`unknown plan mode ${mode}`);
  if (mode === "verify" && !restarts) throw new Error("J4 is not selected, so there is no restart to verify");
  const stages = sequence.map((name) => (name === "restart" ? { phase: "restart", steps: [{ journey: "J4", check: RESTART_STEP }] } : phasePlan(name, selected)));
  const evidence = mode === "all" ? (restarts ? cycleEvidence(out) : [`${out}/results.json`, `${out}/results.md`, `${out}/${screenshotPath("setup")}`])
    : [`${out}/results.json`, `${out}/results.md`, ...(mode === "cleanup" ? [] : [`${out}/${screenshotPath(mode)}`])];
  const journeys = selection ? Object.entries(selection.journeys).map(([id, entry]) => ({ id, status: entry.status, reasons: entry.reasons }))
    : [...selected].map((id) => ({ id, status: "run", reasons: ["full suite: no selection given"] }));
  return { mode, out, target: { instance, port: port ?? null }, journeys, excluded: selection?.excluded_providers ?? [], stages, evidence };
}

export function renderPlan(plan) {
  const port = plan.target.port ?? "resolved from the instance by identify";
  const lines = [`PLAN ${plan.mode} (dry: no instance contacted, nothing written)`, `target: instance=${plan.target.instance} port=${port}`];
  for (const journey of plan.journeys) lines.push(`journey ${journey.id}: ${journey.status === "not_run" ? "not run" : journey.status} (${journey.reasons.length} reason${journey.reasons.length === 1 ? "" : "s"}: ${journey.reasons[0]})`);
  for (const { id } of plan.excluded) lines.push(`provider ${id}: excluded under --waiver (accepted risk)`);
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

export function planFromEnv({ mode, out, instance, port, selection = null }) {
  const parsed = port === undefined || port === "" ? null : Number(port);
  assertIsolated({ instance, port: parsed });
  return buildPlan({ mode, out: resolve(out), instance: instance.trim(), port: parsed, selection });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, ...rest] = process.argv.slice(2);
  const flag = (name) => (rest.includes(name) ? rest[rest.indexOf(name) + 1] : undefined);
  try {
    process.stdout.write(renderPlan(planFromEnv({
      mode, out: flag("--out"), instance: flag("--instance") ?? "", port: flag("--port"),
      selection: selectionFromFlags(loadAreas(), scanSelectionFlags(rest)),
    })));
  } catch (error) {
    console.error(`FAIL ${error.message}`);
    process.exitCode = 7;
  }
}
