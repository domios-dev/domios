import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { dcJson } from "../lib/env.mjs";
import { launchFlags } from "../lib/profile-policy.mjs";
import { workerBrief } from "../lib/fixtures.mjs";
import { promoteFixtureOrchestrator } from "../lib/setup.mjs";
import { connectFixture, installFixture } from "../lib/fixture-control.mjs";
import { attemptBindingVerdict, findTask, isChatBackend, taskVerdict } from "../lib/oracle.mjs";
import { readTab, waitReady, withEvents } from "../lib/tabs.mjs";
import { JourneyFailure, anyOf, observe, touchesPlan, touchesTab } from "../lib/wait.mjs";

const firstTab = (value) => (Array.isArray(value) ? value[0] : value?.tabs?.[0] ?? value);

async function orchestratorFixture(ctx) {
  const { workspace, run } = ctx.state;
  const provider = await installFixture(ctx);
  const created = await dcJson(["agent", "spawn", provider, "--workspace", workspace.id, "--cwd", workspace.cwd,
    "--title", "cjorch", "--non-user-facing", "--description", "Deterministic Plan credential fixture", "--model", "control"]);
  const id = created?.tab_id ?? created?.id;
  if (!id) throw new JourneyFailure(`fixture agent spawn returned no id: ${JSON.stringify(created)}`);
  Object.assign(ctx.state.j3, { orchestrator: id, orchestrator_provider: provider });
  ctx.state.tabs.push({ id, journey: "J3", role: "fixture_orchestrator", control_fixture: true });
  ctx.save();
  await connectFixture(ctx);
  await promoteFixtureOrchestrator(id, ctx.fixtureCommand);
  const team = firstTab(await dcJson(["tab", "inspect", "--tab", id]))?.team_id;
  if (!team) throw new JourneyFailure("set-orchestrator minted no Team");
  ctx.state.j3.team = team;
  ctx.state.teams.push({ leg: "fixture", id: team, orchestrator: id });
  const body = join(ctx.out, "worker-brief.txt");
  writeFileSync(body, workerBrief(run));
  const spec = await ctx.fixtureCommand(["spec", "create", "--title", `core-journeys-${run}`, "--project", workspace.cwd]);
  ctx.state.j3.fixture_spec = spec.spec_id;
  ctx.save();
  await ctx.fixtureCommand(["spec", "section", "add", spec.spec_id, "--title", "Read-only Worker lifecycle", "--body-file", body]);
  const plan = await ctx.fixtureCommand(["plan", "create", "--title", `core-journeys-${run}`, "--from-spec", spec.spec_id, "--project", workspace.cwd]);
  ctx.state.j3.plan = plan?.plan?.id ?? plan?.id;
  ctx.state.plans.push(ctx.state.j3.plan);
  ctx.save();
  await ctx.fixtureCommand(["plan", "goal", "set", ctx.state.j3.plan, "--vector", "The same read-only Worker acknowledges, reports and answers review bounces across restart"]);
  await ctx.fixtureCommand(["plan", "update", ctx.state.j3.plan, "--planner", "none", "--note", "This deterministic single-task lifecycle fixture has no planning work to delegate"]);
  const task = await ctx.fixtureCommand(["plan", "task", "add", ctx.state.j3.plan, "--name", `Journey probe ${run}`,
    "--executor-role", "journey-probe", "--personality", "Probe", "--role-guidance", "Read-only release journey probe",
    "--criterion", "Worker acknowledges and reports in_review", "--test-plan", "none: read-only probe",
    "--correctness", "The same Worker attempt reports after each bounce without changing repository files",
    "--execution-intent", "non_repository", "--body-file", body]);
  ctx.state.j3.task = findTask(task)?.id ?? task?.task?.id ?? task?.id;
  ctx.save();
  await ctx.fixtureCommand(["plan", "approve", ctx.state.j3.plan]);
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

export async function review(ctx, verb, note) {
  const { plan, task, orchestrator } = ctx.state.j3;
  const call = () => (ctx.command ?? ctx.fixtureCommand)(["plan", "review", verb, plan, task, "--note", note], { asTab: orchestrator });
  return verb === "bounce" ? ctx.policy.send(ctx.state.j3.worker, call) : call();
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
    const response = await dispatchWorker(ctx, provider.id, plan, task, orchestrator, workspace);
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
  const rebound = await check("fixture_orchestrator_rebind", async () => {
    const runner = await connectFixture(ctx, { restored: true });
    await ctx.fixtureCommand(["plan", "rebind", j3.plan]);
    return { summary: "fixture resumed with a fresh scoped launch environment", evidence: runner };
  });
  if (!bound || !rebound) return;
  const rechecked = await bounceAndReport(ctx, (name, body) => ctx.check("J3", name, j3.worker, body), "restored_bounce_then_report");
  if (!rechecked) return;
  await ctx.check("J3", "review_accept_done", j3.worker, async () => {
    const response = await review(ctx, "accept", "Release journey accept.");
    const { task } = await waitTask(ctx, { kind: "status", status: "done" }, "accept closes task", ctx.deadlines.ready);
    return { summary: `task ${task.id} done`, evidence: { accept: response } };
  });
}

export function dispatchWorker(ctx, provider, plan, task, orchestrator, workspace) {
  return ctx.policy.launch(provider, "J3-dispatch", (model, profile) => (ctx.command ?? ctx.fixtureCommand)(["plan", "task", "dispatch", plan, task,
    "--agent", provider, ...launchFlags(model, profile), "--workspace", workspace.id, "--cwd", workspace.cwd,
    "--execution-intent", "read_only"], { asTab: orchestrator }));
}
