import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { dcJson } from "../lib/env.mjs";
import { workerBrief } from "../lib/fixtures.mjs";
import { attemptBindingVerdict, findTask, isChatBackend, taskVerdict } from "../lib/oracle.mjs";
import { readTab, waitReady, withEvents } from "../lib/tabs.mjs";
import { JourneyFailure, anyOf, observe, touchesPlan, touchesTab } from "../lib/wait.mjs";

const firstTab = (value) => (Array.isArray(value) ? value[0] : value?.tabs?.[0] ?? value);

async function bindSession(ctx, id, session) {
  const tab = firstTab(await dcJson(["tab", "inspect", "--tab", id]));
  if (!tab?.session_binding_nonce) throw new JourneyFailure(`tab ${id} has no session binding nonce`);
  const response = await ctx.api(`/api/tabs/${id}/hook/session_start`, { method: "POST",
    body: { session_id: session, binding_nonce: tab.session_binding_nonce, source: "core-journeys" } });
  if (response?.ok !== true) throw new JourneyFailure(`session_start not acknowledged: ${JSON.stringify(response)}`);
}

// A shell-tab Orchestrator keeps review deterministic: a live LLM Orchestrator would race the bench's verdicts.
async function orchestratorFixture(ctx) {
  const { workspace, run } = ctx.state;
  const created = await dcJson(["tab", "new", "--workspace", workspace.id, "--cwd", workspace.cwd,
    "--title", "cjorch", "--no-team", "--eager"]);
  const id = created?.tab_id ?? created?.id;
  if (!id) throw new JourneyFailure(`tab new returned no id: ${JSON.stringify(created)}`);
  const session = `cj-orch-${run}`;
  ctx.state.j3 = { orchestrator: id, orchestrator_session: session };
  ctx.state.tabs.push({ id, journey: "J3", role: "fixture_orchestrator", shell: true });
  ctx.save();
  await bindSession(ctx, id, session);
  await dcJson(["agent", "set-orchestrator"], { asTab: id });
  const team = firstTab(await dcJson(["tab", "inspect", "--tab", id]))?.team_id;
  if (!team) throw new JourneyFailure("set-orchestrator minted no Team");
  ctx.state.j3.team = team;
  ctx.state.teams.push({ leg: "fixture", id: team, orchestrator: id });
  const plan = await dcJson(["plan", "create", "--title", `core-journeys-${run}`, "--project", workspace.cwd], { asTab: id });
  ctx.state.j3.plan = plan?.plan?.id ?? plan?.id;
  ctx.state.plans.push(ctx.state.j3.plan);
  const body = join(ctx.out, "worker-brief.txt");
  writeFileSync(body, workerBrief(run));
  const task = await dcJson(["plan", "task", "add", ctx.state.j3.plan, "--name", `Journey probe ${run}`,
    "--executor-role", "journey-probe", "--personality", "Probe", "--role-guidance", "Read-only release journey probe",
    "--criterion", "Worker acknowledges and reports in_review", "--test-plan", "none: read-only probe",
    "--execution-intent", "read_only", "--body-file", body], { asTab: id });
  ctx.state.j3.task = findTask(task)?.id ?? task?.task?.id ?? task?.id;
  ctx.save();
  return ctx.state.j3;
}

function readTask(ctx) {
  const { plan, task } = ctx.state.j3;
  return async () => findTask(await dcJson(["plan", "task", "show", plan, task]));
}

function waitTask(ctx, want, label, deadlineMs = ctx.deadlines.report) {
  const { plan, worker } = ctx.state.j3;
  return withEvents(ctx, deadlineMs, (events) => observe({
    events, filter: anyOf(touchesPlan(plan), touchesTab(worker ?? "")), label,
    read: readTask(ctx), judge: (task) => taskVerdict(task, want),
  }));
}

function review(ctx, verb, note) {
  const { plan, task, orchestrator } = ctx.state.j3;
  return dcJson(["plan", "review", verb, plan, task, "--note", note], { asTab: orchestrator });
}

export async function journeyThree(ctx, provider) {
  const check = (name, body) => ctx.check("J3", name, provider?.id ?? "none", body);
  const fixture = await check("fixture_orchestrator_plan", async () => {
    if (!provider) throw new JourneyFailure("no installed provider runs without a PTY (agent_runtime/chat_backend)");
    const value = await orchestratorFixture(ctx);
    return { summary: `plan ${value.plan} task ${value.task}`, evidence: value };
  });
  if (!fixture) return;
  const dispatched = await check("dispatch_chat_backend_worker", async () => {
    const { plan, task, orchestrator } = ctx.state.j3;
    const { workspace } = ctx.state;
    const response = await dcJson(["plan", "task", "dispatch", plan, task, "--agent", provider.id,
      "--workspace", workspace.id, "--cwd", workspace.cwd, "--execution-intent", "read_only"], { asTab: orchestrator });
    const worker = response?.tab_id;
    if (!worker) throw new JourneyFailure(`dispatch returned no Worker tab: ${JSON.stringify(response).slice(0, 400)}`);
    ctx.state.j3.worker = worker;
    ctx.state.tabs.push({ id: worker, provider: provider.id, journey: "J3", role: "worker" });
    ctx.save();
    const ready = await waitReady(ctx, worker);
    if (!isChatBackend(ready)) throw new JourneyFailure(`Worker backing is ${ready.backing?.kind ?? "unknown"}, expected chat_backend`);
    return { summary: `${worker} ${ready.backing.kind}`, evidence: { dispatch: response, backing: ready.backing } };
  });
  if (!dispatched) return;
  const acked = await check("worker_ack", async () => {
    const { attempt } = await waitTask(ctx, { kind: "acked" }, "Worker ACK");
    ctx.state.j3.attempt = attempt.id;
    ctx.save();
    return { summary: `attempt ${attempt.id} acknowledged`, evidence: { attempt: attempt.id, acknowledged_at: attempt.acknowledged_at } };
  });
  if (!acked) return;
  const reported = await check("worker_report_in_review", async () => {
    const { attempt } = await waitTask(ctx, { kind: "reported" }, "Worker report in_review");
    ctx.state.j3.reported_at = attempt.reported_at;
    ctx.save();
    return { summary: `reported at ${attempt.reported_at}`, evidence: { reported_at: attempt.reported_at } };
  });
  if (!reported) return;
  await bounceAndReport(ctx, check, "bounce_then_report");
}

async function bounceAndReport(ctx, check, name) {
  return check(name, async () => {
    const response = await review(ctx, "bounce", "Release journey bounce: report again with status in_review.");
    const { attempt } = await waitTask(ctx, { kind: "reported", after: ctx.state.j3.reported_at }, "Worker re-report after bounce");
    ctx.state.j3.reported_at = attempt.reported_at;
    ctx.save();
    return { summary: `bounced and re-reported at ${attempt.reported_at}`, evidence: { bounce: response, attempt: attempt.id } };
  });
}

export async function journeyThreeAfterRestart(ctx) {
  const j3 = ctx.state.j3;
  if (!j3?.worker || !j3.reported_at) return;
  const check = (name, body) => ctx.check("J4", name, j3.worker, body);
  const bound = await check("restored_worker_attempt_binding", async () => {
    const record = await waitReady(ctx, j3.worker, { requireSession: true });
    if (!isChatBackend(record)) throw new JourneyFailure(`restored Worker backing ${record.backing?.kind}, expected chat_backend`);
    const task = await readTask(ctx)();
    const verdict = attemptBindingVerdict(await readTab(ctx.api, j3.worker), task);
    if (verdict.fail) throw new JourneyFailure(verdict.fail, { task_binding: record.task_binding });
    return { summary: `attempt ${verdict.done.attempt_id}`, evidence: verdict.done };
  });
  await check("fixture_orchestrator_rebind", async () => {
    await bindSession(ctx, j3.orchestrator, j3.orchestrator_session);
    return "fixture Orchestrator session rebound";
  });
  if (!bound) return;
  const rechecked = await bounceAndReport(ctx, (name, body) => ctx.check("J3", name, j3.worker, body), "restored_bounce_then_report");
  if (!rechecked) return;
  await ctx.check("J3", "review_accept_done", j3.worker, async () => {
    const response = await review(ctx, "accept", "Release journey accept.");
    const { task } = await waitTask(ctx, { kind: "status", status: "done" }, "accept closes task", ctx.deadlines.ready);
    return { summary: `task ${task.id} done`, evidence: { accept: response } };
  });
}
