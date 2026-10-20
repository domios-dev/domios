import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { client, dcJson, target } from "./lib/env.mjs";
import { runId } from "./lib/fixtures.mjs";
import { planFromEnv, renderPlan } from "./lib/plan.mjs";
import { captureScreenshot } from "./lib/screenshots.mjs";
import { REQUIRED_PROVIDERS } from "./lib/oracle.mjs";
import { loadResults, recorder, saveResults, verdict } from "./lib/results.mjs";
import { JourneyFailure } from "./lib/wait.mjs";
import { cleanup } from "./journeys/cleanup.mjs";
import { journeyThree, journeyThreeAfterRestart } from "./journeys/dispatch.mjs";
import { restartCheck, restoredSends, restoredTabsReady, snapshotBeforeRestart } from "./journeys/restore.mjs";
import { journeyOne, providers } from "./journeys/spawn.mjs";
import { apiNoContext, uiContext } from "./journeys/team.mjs";

export function parseArgs(argv) {
  const [mode, ...rest] = argv;
  if (!["setup", "verify", "cleanup"].includes(mode)) throw new Error(`unknown driver mode ${mode}`);
  const options = { mode, out: null, providers: null, allTabs: false, historyBytes: 320 * 1024, deadlineSecs: 240, keep: false, expectBuildChange: false, plan: false };
  for (let index = 0; index < rest.length; index += 1) {
    const flag = rest[index], value = rest[index + 1];
    if (flag === "--out") { options.out = resolve(value); index += 1; }
    else if (flag === "--providers") { options.providers = value.split(",").map((id) => id.trim()).filter(Boolean); index += 1; }
    else if (flag === "--history-kb") { options.historyBytes = Number(value) * 1024; index += 1; }
    else if (flag === "--deadline-secs") { options.deadlineSecs = Number(value); index += 1; }
    else if (flag === "--all-tabs") options.allTabs = true;
    else if (flag === "--keep") options.keep = true;
    else if (flag === "--expect-build-change") options.expectBuildChange = true;
    else if (flag === "--plan") options.plan = true;
    else throw new Error(`unknown driver flag ${flag}`);
  }
  if (!options.out) throw new Error("--out DIR is required");
  if (!Number.isFinite(options.historyBytes) || options.historyBytes < 0) throw new Error("--history-kb must be a number");
  if (!(options.deadlineSecs > 0)) throw new Error("--deadline-secs must be positive");
  return options;
}

export function preferred(run, wantRuntime) {
  const pool = run.filter((provider) => provider.runtime === wantRuntime);
  return pool.sort((a, b) => rank(a.id) - rank(b.id))[0] ?? null;
}
const rank = (id) => (REQUIRED_PROVIDERS.includes(id) ? REQUIRED_PROVIDERS.indexOf(id) : REQUIRED_PROVIDERS.length);

function serial() {
  let tail = Promise.resolve();
  return (body) => {
    const next = tail.then(body, body);
    tail = next.catch(() => {});
    return next;
  };
}

async function provisionWorkspace(ctx) {
  return ctx.check("SETUP", "workspace_provisioned", ctx.state.run, async () => {
    const workspace = await dcJson(["workspace", "create", `CJ-${ctx.state.run}`, "--provision"]);
    if (!workspace?.id || !workspace?.cwd) throw new JourneyFailure(`workspace create returned ${JSON.stringify(workspace)}`);
    ctx.state.workspace = { id: workspace.id, cwd: workspace.cwd };
    ctx.save();
    const git = (...args) => execFileSync("git", ["-C", workspace.cwd, ...args], { stdio: "ignore", timeout: 30_000 });
    git("init", "-q");
    git("-c", "user.name=core-journeys", "-c", "user.email=core-journeys@localhost", "commit", "-q", "--allow-empty", "-m", "core-journeys fixture");
    return { summary: `${workspace.id} ${workspace.cwd}`, evidence: ctx.state.workspace };
  });
}

async function setup(ctx) {
  ctx.state.run ??= runId();
  Object.assign(ctx.state, { tabs: ctx.state.tabs ?? [], teams: ctx.state.teams ?? [], plans: ctx.state.plans ?? [] });
  if (!(await provisionWorkspace(ctx))) return;
  const selected = await providers(ctx);
  const pty = preferred(selected.run, false), runtime = preferred(selected.run, true);
  const ui = serial();
  await Promise.all([
    journeyOne(ctx, selected),
    (async () => {
      if (!pty) return ctx.check("J2", "api_no_context", "none", async () => { throw new JourneyFailure("no installed PTY provider"); });
      await apiNoContext(ctx, pty.id);
      const manager = ctx.state.teams.find((team) => team.leg === "api_no_context")?.orchestrator;
      await ui(() => uiContext(ctx, { leg: "ui_manager_context", contextTab: manager, provider: pty.id }));
    })(),
    (async () => {
      await journeyThree(ctx, runtime);
      const provider = runtime?.id ?? pty?.id ?? "none";
      await ui(() => uiContext(ctx, { leg: "ui_worker_context", contextTab: ctx.state.j3?.worker, provider }));
    })(),
  ]);
  await ctx.check("J4", "pre_restart_snapshot", ctx.state.run, async () => {
    await snapshotBeforeRestart(ctx);
    return `${ctx.state.before_restart.tabs.length} agent tabs recorded`;
  });
  await captureScreenshot(ctx, "setup");
}

async function verify(ctx) {
  if (!ctx.state.run) throw new Error("verify needs the state written by setup in the same --out directory");
  await restartCheck(ctx);
  await restoredTabsReady(ctx);
  await Promise.all([restoredSends(ctx), journeyThreeAfterRestart(ctx)]);
  await captureScreenshot(ctx, "verify");
}

export async function main(argv) {
  const options = parseArgs(argv);
  if (options.plan) {
    process.stdout.write(renderPlan(planFromEnv({
      mode: options.mode, out: options.out, instance: process.env.CODETERM_INSTANCE ?? "", port: process.env.CODETERM_API_PORT,
    })));
    return 0;
  }
  mkdirSync(options.out, { recursive: true });
  const dev = target();
  const { api } = client(dev);
  const results = loadResults(options.out);
  const health = await api("/api/health");
  const started = performance.now();
  const phase = { name: options.mode, instance: dev.instance, port: dev.port, build_sha: health.build_sha ?? health.build, started_at: new Date().toISOString() };
  results.phases.push(phase);
  const deadlineMs = options.deadlineSecs * 1000;
  const ctx = {
    dev, api, out: options.out, options, health, state: results.state,
    deadlines: { ready: deadlineMs, reply: deadlineMs, report: deadlineMs + 120_000, ui: 30_000 },
    save: () => saveResults(options.out, results),
  };
  ctx.check = recorder(options.out, results);
  try {
    if (options.mode === "setup") await setup(ctx);
    if (options.mode === "verify") await verify(ctx);
    if (options.mode === "cleanup" && !options.keep) await cleanup(ctx);
  } catch (error) {
    await ctx.check("HARNESS", options.mode, null, async () => { throw error; });
  } finally {
    phase.wall_ms = Math.round(performance.now() - started);
    saveResults(options.out, results);
  }
  const outcome = verdict(results);
  console.log(`${options.mode.toUpperCase()} ${outcome.pass ? "PASS" : "FAIL"}: ${outcome.reason}; phase wall ${(phase.wall_ms / 1000).toFixed(1)}s; results ${options.out}/results.md`);
  return outcome.pass ? 0 : 1;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then((code) => { process.exitCode = code; }, (error) => {
    console.error(`HARNESS FAIL: ${error.stack ?? error.message}`);
    process.exitCode = 2;
  });
}
