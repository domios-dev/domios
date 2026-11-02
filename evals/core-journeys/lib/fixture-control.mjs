import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, realpathSync, renameSync, watch, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { dcJson, root } from "./env.mjs";
import { readTab, waitReady } from "./tabs.mjs";

export const fixtureProviderId = run => `cj-orchestrator-${String(run).toLowerCase()}`;
const here = dirname(fileURLToPath(import.meta.url));

function awaitJson(path, judge, deadlineMs, signal) {
  return new Promise((accept, reject) => {
    const controller = new AbortController();
    let settled = false;
    const timer = setTimeout(() => finish(new Error(`deadline: fixture receipt ${path}; do not replay the request`)), deadlineMs);
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer); controller.abort();
      error ? reject(error) : accept(value);
    };
    const check = () => {
      try {
        const value = JSON.parse(readFileSync(path, "utf8"));
        if (judge(value)) finish(null, value);
      } catch (error) { if (error.code !== "ENOENT" && !(error instanceof SyntaxError)) finish(error); }
    };
    const watcher = watch(dirname(path), { signal: controller.signal }, (_event, name) => {
      if (name?.toString() === path.split(/[\\/]/).at(-1)) check();
    });
    watcher.on("error", error => finish(error));
    signal?.addEventListener("abort", () => finish(signal.reason), { once: true, signal: controller.signal });
    check();
  });
}

export function createFixtureBundle(ctx) {
  const id = fixtureProviderId(ctx.state.run);
  const bundle = join(ctx.out, "fixture-provider"), sessions = join(bundle, "sessions");
  const discovery = join(ctx.out, "fixture-runner.json");
  mkdirSync(sessions, { recursive: true });
  const quote = value => process.platform === "win32" ? `'${String(value).replaceAll("'", "''")}'` : `'${String(value).replaceAll("'", "'\"'\"'")}'`;
  const argv = [process.execPath, join(here, "fixture-runner.mjs"), "--provider", id, "--discovery", discovery,
    "--sessions", sessions, "--bench", join(root, "evals/core-journeys/run.sh"), "--checkout", process.env.CORE_JOURNEY_BUILD_CHECKOUT || root,
    "--instance", ctx.dev.instance, "--port", String(ctx.dev.port)];
  const command = `${process.platform === "win32" ? "& " : ""}${argv.map(quote).join(" ")}`;
  const template = readFileSync(join(here, "fixture-provider.cjs"), "utf8");
  writeFileSync(join(bundle, "plugin.js"), template.replace("__FIXTURE_SETTINGS__", JSON.stringify({ command, sessions: sessions.replaceAll("\\", "/") })));
  writeFileSync(join(bundle, "plugin.json"), JSON.stringify({
    id, version: "0.0.0", hostApi: 2, displayName: "Core journeys control fixture", entry: "plugin.js",
    // Launch preflight resolves binaryName on PATH; the fixture runs under Node.
    cliBinary: process.execPath, binaryName: "node", capabilities: { agentProvider: true },
    models: [{ id: "control", displayName: "Deterministic control" }], defaultModel: "control",
    modelCatalogue: { authority: "authoritative", liveDiscoveryRequired: false },
    sessions: { rootDir: sessions.replaceAll("\\", "/"), cwdEncoding: "none", fileExtension: "jsonl" },
    spawn: { readinessStrategy: { kind: "poll_shell_prompt", timeoutSecs: 30 }, taskDelivery: "inline_flag",
      verifySubmit: { kind: "none" }, sessionIdInjection: "fresh", hasHooks: true,
      launchPermissions: { unattendedModeModels: ["control"] } },
  }, null, 2));
  return { id, bundle, discovery };
}

export async function installFixture(ctx) {
  const id = fixtureProviderId(ctx.state.run);
  if ((await ctx.api("/api/agents/providers")).some(provider => provider.id === id)) throw new Error("Fixture provider id is already registered; refusing replacement");
  const { bundle, discovery } = createFixtureBundle(ctx);
  ctx.state.j3 = { fixture_provider: id, fixture_discovery: discovery };
  ctx.save();
  const validation = await dcJson(["plugin", "validate", bundle]);
  const installed = await dcJson(["plugin", "install", "--from-dir", bundle]);
  if (installed.provider_registration !== "acknowledged") throw new Error("Fixture provider registration was not acknowledged");
  ctx.state.j3.fixture_validation = validation;
  ctx.save();
  return id;
}

export async function connectFixture(ctx, { restored = false } = {}) {
  const j3 = ctx.state.j3;
  const tab = await waitReady(ctx, j3.orchestrator, { requireSession: true });
  if (tab.agent_type !== j3.fixture_provider || tab.reported_session_binding_nonce !== tab.session_binding_nonce) throw new Error("Fixture launch binding is not current");
  const runner = await awaitJson(j3.fixture_discovery, value => value.tab_id === tab.id && value.session_id === tab.agent_session_id
    && value.binding_nonce === tab.session_binding_nonce, ctx.deadlines.ready);
  const expected = resolve(ctx.dev.dataDir, "runtime/agent-pads", tab.id, "cj-runner", tab.session_binding_nonce);
  if (realpathSync(runner.queue) !== realpathSync(expected)) throw new Error("Fixture queue is outside its own launch pad");
  if (restored && (runner.binding_nonce === j3.runner?.binding_nonce || runner.session_id !== j3.orchestrator_session)) throw new Error("Fixture did not reacquire its own launch environment after restart");
  j3.runner = runner;
  j3.orchestrator_session = tab.agent_session_id;
  ctx.save();
  ctx.fixtureCommand = async args => {
    const current = await readTab(ctx.api, j3.orchestrator);
    if (current.session_binding_nonce !== runner.binding_nonce || current.agent_session_id !== runner.session_id) throw new Error("Fixture launch changed; request was not submitted");
    const id = randomUUID();
    const path = join(runner.queue, "requests", `${id}.request`);
    const resultPath = join(runner.queue, "results", `${id}.json`);
    const cancellation = new AbortController();
    const receipt = awaitJson(resultPath, value => value.id === id && value.tab_id === current.id
      && value.binding_nonce === runner.binding_nonce && value.session_id === runner.session_id, ctx.deadlines.report, cancellation.signal);
    const request = { id, tab_id: current.id, binding_nonce: runner.binding_nonce, session_id: runner.session_id, args: [...args, "--json"] };
    try {
      writeFileSync(`${path}.next`, JSON.stringify(request), { mode: 0o600 });
      renameSync(`${path}.next`, path);
    } catch (error) {
      cancellation.abort(error);
      await receipt.catch(() => {});
      throw error;
    }
    const result = await receipt;
    if (result.code !== 0) throw new Error(`Fixture dc exit ${result.code}: ${(result.stdout || result.stderr).slice(0, 800)}`);
    return JSON.parse(result.stdout).result;
  };
  return runner;
}
