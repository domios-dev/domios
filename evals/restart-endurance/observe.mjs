import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { exactReply, firstTurnPolicy, kind, probeReady } from "./oracle.mjs";
import { digest, logLines, newEvidence } from "./logs.mjs";

const script = fileURLToPath(import.meta.url);
const root = resolve(dirname(script), "../..");
const [mode, output, roundArgument] = process.argv.slice(2);
const round = Number(roundArgument ?? 0);
const out = resolve(output);
const API = "http://127.0.0.1:7686";
const deadlineMs = 120_000;
const dataDir = process.env.CODETERM_DEV_DATA_DIR;
if (process.env.CODETERM_API_PORT !== "7686" || process.env.CODETERM_INSTANCE || !dataDir) {
  throw new Error("Run through the default DEV lib dev_run wrapper");
}
const headers = {
  Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}`,
  "Content-Type": "application/json",
};
const resultPath = join(out, "results.json");
let result = existsSync(resultPath) ? JSON.parse(readFileSync(resultPath, "utf8")) : {
  schema: 1, started_at: new Date().toISOString(), rounds: [], fixtures: [], coverage: [],
};

function save() {
  writeFileSync(`${resultPath}.tmp`, `${JSON.stringify(result, null, 2)}\n`);
  renameSync(`${resultPath}.tmp`, resultPath);
  const cell = (value) => String(value ?? "—").replaceAll("|", "\\|").replaceAll("\n", " ");
  const lines = ["# Restart endurance", "", `Started: ${result.started_at}`, "",
    `Build: ${result.health?.build_sha ?? result.health?.build ?? "unknown"}`, "",
    "General siblings use the sidebar API and reside in General, as required by that API.", "",
    "## Coverage", "", ...result.coverage.map((entry) => `- ${cell(entry.provider)}: ${cell(entry.status)} ${cell(entry.reason ?? "")}`), ""];
  if (result.fatal) lines.push(`Harness stopped: ${cell(result.fatal)}`, "");
  for (const entry of result.rounds) {
    lines.push(`## Round ${entry.round}`, "", "| Tab | Cohort | Provider | Boot stage | Identity | Session before → after | Exact answer | ms | Verdict / reason |",
      "| --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    for (const row of entry.rows) lines.push(`| ${[
      row.id ?? "spawn refused", row.cohort, row.provider, row.before_probe?.boot_stage,
      row.before_probe?.agent_type, `${row.previous_session ?? "—"} → ${row.after_probe?.agent_session_id ?? row.before_probe?.agent_session_id ?? "—"}`,
      row.answer ?? "—", row.elapsed_ms, `${row.pass ? "PASS" : row.observed_only ? "OBSERVED" : "FAIL"}: ${row.reason ?? ""}`,
    ].map(cell).join(" | ")} |`);
    lines.push("", `Log evidence: [round-${entry.round}.log](round-${entry.round}.log). Full tab records, lifecycle and transcripts are in results.json.`, "");
    for (const row of entry.rows.filter((row) => !row.pass && !row.observed_only)) {
      lines.push(`### ${cell(row.id ?? row.provider)} failure`, "", `${cell(row.reason)}`, "", "```text");
      if (!row.log_evidence?.length) lines.push("No tab-specific restore/launch log line retained; cause unproven.");
      for (const line of row.log_evidence ?? []) lines.push(line);
      lines.push("```", "");
    }
  }
  writeFileSync(join(out, "results.md"), `${lines.join("\n")}\n`);
}

async function api(path, method = "GET", body, signal = AbortSignal.timeout(30_000)) {
  const until = Date.now() + deadlineMs;
  for (;;) {
    const response = await fetch(`${API}${path}`, {
      method, headers, body: body === undefined ? undefined : JSON.stringify(body), signal,
    });
    const text = await response.text();
    // A read-only snapshot answers 503 while restore settles; retry only idempotent GETs.
    if (response.status === 503 && method === "GET" && Date.now() < until) {
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 1_000));
      continue;
    }
    if (!response.ok) throw new Error(`${method} ${path}: HTTP ${response.status} ${text}`);
    return text ? JSON.parse(text) : null;
  }
}

async function command(args) {
  return new Promise((accept, reject) => {
    const child = spawn(process.env.ENDURE_BASH ?? "bash", args, {
      cwd: root, stdio: ["ignore", "pipe", "pipe"], signal: AbortSignal.timeout(deadlineMs),
    });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => code === 0 ? accept(stdout.trim()) : reject(new Error(`CLI exit ${code}: ${stdout} ${stderr}`)));
  });
}
async function dc(...args) {
  return command([join(root, "evals/restart-endurance/run.sh"), "--dc", ...args]);
}
const unwrap = (value) => value?.data ?? value;
async function dcJson(...args) { return unwrap(JSON.parse(await dc(...args, "--json"))); }

class Events {
  constructor(signal) {
    this.signal = signal;
    this.queue = [];
    this.waiter = null;
    this.failure = null;
    this.controller = new AbortController();
  }
  async open() {
    const response = await fetch(`${API}/api/events/stream`, {
      headers, signal: AbortSignal.any([this.signal, this.controller.signal]),
    });
    if (!response.ok || !response.body) throw new Error(`SSE HTTP ${response.status}`);
    this.reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    this.consume().catch((error) => {
      this.failure = error;
      this.waiter?.reject(error);
      this.waiter = null;
    });
  }
  async consume() {
    let buffer = "";
    for (;;) {
      const { done, value } = await this.reader.read();
      if (done) throw new Error("SSE closed before observation finished");
      buffer += value.replaceAll("\r", "");
      let boundary;
      while ((boundary = buffer.indexOf("\n\n")) >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const data = frame.split("\n").filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart()).join("\n");
        if (!data) continue;
        const event = JSON.parse(data);
        if (event.event_type === "overflow") throw new Error("SSE overflow: observation incomplete");
        if (this.waiter) {
          const waiter = this.waiter;
          this.waiter = null;
          waiter.resolve(event);
        } else {
          this.queue.push(event);
          if (this.queue.length > 10_000) throw new Error("SSE queue exceeded observation bound");
        }
      }
    }
  }
  next() {
    if (this.failure) return Promise.reject(this.failure);
    if (this.signal.aborted) return Promise.reject(new Error("Observation deadline exceeded"));
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    return new Promise((accept, reject) => {
      const onAbort = () => reject(new Error("Observation deadline exceeded"));
      this.signal.addEventListener("abort", onAbort, { once: true });
      const settle = (fn, value) => {
        this.signal.removeEventListener("abort", onAbort);
        fn(value);
      };
      this.waiter = { resolve: (event) => settle(accept, event), reject: (error) => settle(reject, error) };
    });
  }
  close() { this.controller.abort(); }
}

const touches = (event, id) => event.data?.tab_id === id || event.event_type?.includes(id) || event.event_type === "mux-state-changed";
const terminalStages = new Set(["spawn_failed", "launch_deadline_elapsed", "launch_binding_unconfirmed", "awaiting_owner_prompt"]);
async function tabs(signal) {
  const [summaries, state] = await Promise.all([
    api("/api/tabs?local_only=true&all_workspaces=true", "GET", undefined, signal),
    api("/api/mux/state", "GET", undefined, signal),
  ]);
  const full = Object.fromEntries(Object.values(state.tabs ?? {}).map((record) => [record.id, record]));
  return summaries.map((summary) => ({ ...summary, ...full[summary.id] }));
}
async function tab(id, signal) { return (await tabs(signal)).find((record) => record.id === id); }
function failure(record) {
  if (!record) return "Tab absent from the all-tabs projection";
  if (terminalStages.has(record.boot_stage)) return `${record.boot_stage}: ${record.boot_failure_reason ?? JSON.stringify(record.pending_prompt ?? null)}`;
  return null;
}
async function waitReady(events, id, signal, policy, requireSession = false) {
  for (;;) {
    const record = await tab(id, signal);
    const reason = failure(record);
    if (reason) throw new Error(reason);
    if (probeReady(record, policy, requireSession)) return record;
    let event;
    do { event = await events.next(); } while (!touches(event, id));
  }
}
async function transcript(id, signal) {
  return api(`/api/tabs/${id}/chat`, "GET", undefined, signal);
}
async function paddedPrompt(id, marker) {
  const pad = await dc("--as-tab", id, "pad", "path");
  const path = join(pad, `endure-${marker}.txt`);
  writeFileSync(path, `Reply with exactly ${marker}`, "utf8");
  return readFileSync(path, "utf8");
}

async function probe(fixture, n, observeOnly = false) {
  const row = { id: fixture.id, cohort: fixture.cohort, provider: fixture.provider,
    previous_session: fixture.last_session ?? null, pass: false, observed_only: observeOnly };
  if (!fixture.id) { row.reason = fixture.spawn_error; return row; }
  const start = performance.now();
  const signal = AbortSignal.timeout(deadlineMs);
  const events = new Events(signal);
  let subscribed = false;
  try {
    await events.open();
    row.initial = await tab(fixture.id, signal);
    if (observeOnly) {
      row.before_probe = row.initial;
      row.reason = failure(row.initial) ?? "Existing tab inspected; no message authorized outside Endure fixtures";
      return row;
    }
    row.phase = "waiting_for_input_readiness";
    const ready = await waitReady(events, fixture.id, signal, fixture.first_turn_policy, Boolean(fixture.last_session));
    row.before_probe = ready;
    if (ready.agent_type !== fixture.provider) throw new Error(`Provider identity changed: ${fixture.provider} → ${ready.agent_type}`);
    if (fixture.last_session && fixture.last_session !== ready.agent_session_id) throw new Error(`Session changed before probe: ${fixture.last_session} → ${ready.agent_session_id}`);
    const before = await transcript(fixture.id, signal);
    const baseline = before.messages.length;
    const marker = `ENDURE_${fixture.provider}_${n}`;
    row.expected = marker;
    row.prompt = await paddedPrompt(fixture.id, marker);
    row.send_started_at = new Date().toISOString();
    row.phase = "sending";
    row.send_response = await api(`/api/tabs/${fixture.id}/send`, "POST", { text: row.prompt }, signal);
    row.phase = "waiting_for_session_binding";
    const bound = await waitReady(events, fixture.id, signal, fixture.first_turn_policy, true);
    row.bound_session = bound.agent_session_id;
    if (bound.agent_type !== fixture.provider || (ready.agent_session_id && ready.agent_session_id !== bound.agent_session_id)) {
      throw new Error("Provider or session changed while establishing probe binding");
    }
    await api(`/api/tabs/${fixture.id}/chat-raw/start`, "POST", { subscriptionId: `endure-${fixture.id}-${n}` }, signal);
    subscribed = true;
    row.phase = "waiting_for_exact_reply";
    for (;;) {
      const snapshot = await transcript(fixture.id, signal);
      row.transcript = snapshot;
      const answer = exactReply(snapshot.messages, baseline, row.prompt, marker);
      const current = await tab(fixture.id, signal);
      if (answer && current?.status === "idle") {
        row.answer = answer;
        row.reply_ms = Math.round(performance.now() - start);
        row.after_probe = current;
        if (row.after_probe?.agent_session_id !== bound.agent_session_id) throw new Error("Session changed during probe; reply ownership uncertain");
        if (fixture.last_session && fixture.last_session !== ready.agent_session_id) {
          row.reason = `Session changed across restart: ${fixture.last_session} → ${ready.agent_session_id}`;
        } else { row.pass = true; }
        fixture.last_session = row.after_probe.agent_session_id;
        row.phase = "answered";
        break;
      }
      if (failure(current)) throw new Error(failure(current));
      let event;
      do { event = await events.next(); } while (!touches(event, fixture.id));
    }
  } catch (error) {
    row.reason = error.message;
  } finally {
    row.elapsed_ms = Math.round(performance.now() - start);
    events.close();
    row.after_probe ??= await tab(fixture.id).catch(() => null);
    row.before_probe ??= row.initial ?? row.after_probe;
    row.lifecycle = await dcJson("tab", "inspect", "--tab", fixture.id, "--lifecycle").catch((error) => ({ error: error.message }));
    if (subscribed) await api(`/api/tabs/${fixture.id}/chat-raw/stop`, "POST", { subscriptionId: `endure-${fixture.id}-${n}` }).catch((error) => { row.subscription_stop_error = error.message; });
    console.log(`${observeOnly ? "OBSERVED" : row.pass ? "PASS" : "FAIL"} round=${n} tab=${fixture.id} provider=${fixture.provider} ${row.reason ?? row.answer ?? ""}`);
  }
  return row;
}

async function checkHealth() {
  const health = await api("/api/health");
  if (Number(health.instance?.api_port) !== 7686 || health.instance?.name) throw new Error("Health does not identify default DEV 7686");
  const head = await command(["-c", "git rev-parse HEAD"]);
  const build = health.build_sha ?? health.build;
  if (!build) throw new Error("DEV health reports no build SHA");
  if (!head.startsWith(build)) {
    const backend = await command(["-c", `git merge-base --is-ancestor ${build} HEAD && git diff --name-only ${build} HEAD -- src-tauri cli plugins packages/plugin-sdk || echo NOT_ANCESTOR`]);
    if (backend.trim()) throw new Error(`DEV build ${build} differs from HEAD ${head} in backend or plugin files; build once before the eval`);
  }
  return health;
}

async function setup() {
  result.health = await checkHealth();
  const providers = await dcJson("agent", "providers");
  result.provider_descriptors = providers;
  const inventory = await dcJson("plugin", "list");
  const policies = new Map();
  for (const plugin of inventory.plugins ?? []) {
    const directory = plugin.install_path ?? plugin.path;
    if (!directory) continue;
    const path = join(directory, "plugin.json");
    if (!existsSync(path)) continue;
    const manifest = JSON.parse(readFileSync(path, "utf8"));
    if (manifest.id !== plugin.id) throw new Error(`Plugin inventory/manifest mismatch: ${plugin.id}`);
    policies.set(plugin.id, { ...firstTurnPolicy(manifest), manifest_path: path });
  }
  const workspaces = await dcJson("workspace", "list");
  if (workspaces.some((workspace) => workspace.name === "Endure")) throw new Error("Endure already exists; preserve it and review the previous fixture manifest before explicitly closing it");
  const records = await tabs();
  const watched = new Set(workspaces.filter((workspace) => ["General", "Rcqa"].includes(workspace.name)).map((workspace) => workspace.id));
  result.existing = records.filter((record) => watched.has(record.workspace_id)).map((record) => ({
    id: record.id, provider: record.agent_type, cohort: "existing", baseline: record,
  }));
  const config = await dcJson("config", "get");
  const general = records.find((record) => record.protected && record.role_profile === "general_agent") ?? records.find((record) => record.protected && record.agent_type);
  if (!general || !config.general_agent?.first_run_complete) throw new Error("General must have completed Get started; eval does not change global setup");
  const workspace = await dcJson("workspace", "create", "Endure", "--provision");
  result.workspace = workspace;
  save();
  for (const cohort of ["fresh", "used"]) {
    for (const provider of ["codex", "claude", "opencode", "grok", "aider"]) {
      const descriptor = providers.find((candidate) => candidate.id === provider);
      if (!descriptor || descriptor.availability === "unavailable" || descriptor.cli_status?.installed === false) {
        result.coverage.push({ provider, cohort, status: provider === "aider" ? "optional unavailable" : "required unavailable", reason: descriptor ? JSON.stringify(descriptor.cli_status ?? descriptor.availability) : "not registered" });
        save();
        continue;
      }
      result.coverage.push({ provider, cohort, status: "launch requested" });
      const fixture = { provider, cohort, first_turn_policy: policies.get(provider) ?? firstTurnPolicy(null) };
      result.fixtures.push(fixture);
      save();
      try {
        const spawned = await dcJson("agent", "spawn", provider, "--workspace", workspace.id,
          "--cwd", workspace.cwd, "--title", `${provider}${cohort === "fresh" ? "F" : "U"}`,
          "--description", `Restart endurance ${cohort} fixture`, "--non-user-facing");
        fixture.id = spawned.tab_id;
        if (!fixture.id) throw new Error(`Spawn did not return tab_id: ${JSON.stringify(spawned)}`);
      } catch (error) { fixture.spawn_error = error.message; }
      save();
    }
    for (let sibling = 1; sibling <= 2; sibling++) {
      const provider = config.general_agent.agent_provider;
      const fixture = { provider, cohort: `general-${cohort}`, sibling, first_turn_policy: policies.get(provider) ?? firstTurnPolicy(null) };
      result.fixtures.push(fixture);
      save();
      try {
        const spawned = await api("/api/agents/general/sessions", "POST", {
          workspaceId: general.workspace_id, providerId: fixture.provider, cwd: general.cwd,
          role: "general_agent", intent: "fresh_general",
        });
        fixture.id = spawned.tab_id;
        if (!fixture.id) throw new Error("General spawn did not return tab_id");
      } catch (error) { fixture.spawn_error = error.message; }
      save();
    }
  }
  const entry = { round: 0, rows: [] };
  result.rounds.push(entry);
  for (const fixture of result.fixtures) {
    if (fixture.cohort.endsWith("used")) entry.rows.push(await probe(fixture, 0));
    else if (fixture.id) {
      const signal = AbortSignal.timeout(deadlineMs);
      const events = new Events(signal);
      let record, reason, snapshot;
      try {
        await events.open();
        record = await waitReady(events, fixture.id, signal, fixture.first_turn_policy);
        snapshot = await transcript(fixture.id, signal);
        if (snapshot.messages.some((message) => ["user", "assistant"].includes(kind(message)))) reason = "Fresh fixture has conversation turns before any eval probe";
        fixture.empty_session_id = record.agent_session_id;
        fixture.last_session = null;
      } catch (error) { reason = error.message; }
      finally { events.close(); }
      entry.rows.push({ ...fixture, before_probe: record, transcript: snapshot, pass: !reason, reason: reason ?? "No user or assistant turn before restart" });
    } else entry.rows.push({ ...fixture, pass: false, reason: fixture.spawn_error });
    save();
  }
  result.setup_complete = true;
  save();
}

function captureLogs(n) {
  const lines = newEvidence(logLines(join(dataDir, "logs")), result.log_baseline ?? []);
  writeFileSync(join(out, `round-${n}.log`), `${lines.join("\n")}\n`);
  return lines;
}

async function runRound(n) {
  const health = await checkHealth();
  const restored = await tabs();
  let workspace;
  const readiness = [];
  for (const fixture of result.fixtures.filter((fixture) => fixture.id)) {
    const signal = AbortSignal.timeout(deadlineMs);
    const events = new Events(signal);
    try {
      await events.open();
      const record = restored.find((record) => record.id === fixture.id);
      if (record && workspace !== record.workspace_id) {
        await dc("workspace", "focus", record.workspace_id);
        workspace = record.workspace_id;
      }
      await dc("tab", "focus", "--tab", fixture.id);
      readiness.push(await waitReady(events, fixture.id, signal, fixture.first_turn_policy));
    } catch (error) {
      readiness.push({ id: fixture.id, readiness_error: error.message });
    } finally { events.close(); }
  }
  const entry = { round: n, health, rows: [], started_at: result.round_started_at };
  entry.restore_ready_at = new Date().toISOString();
  entry.restore_readiness = readiness;
  result.rounds.push(entry);
  save();
  for (const fixture of result.fixtures) {
    const failedReadiness = readiness.find((record) => record.id === fixture.id)?.readiness_error;
    entry.rows.push(failedReadiness
      ? { id: fixture.id, cohort: fixture.cohort, provider: fixture.provider, pass: false, reason: failedReadiness, phase: "restore_readiness_failed" }
      : await probe(fixture, n));
    save();
  }
  for (const fixture of result.existing) {
    entry.rows.push(await probe(fixture, n, true));
    save();
  }
  const lines = captureLogs(n);
  for (const row of entry.rows) row.log_evidence = row.id ? lines.filter((line) => line.includes(row.id)) : [];
  const owners = new Map();
  for (const row of entry.rows.filter((row) => !row.observed_only)) {
    const session = row.after_probe?.agent_session_id;
    if (!session) continue;
    if (owners.has(session)) {
      const other = owners.get(session);
      row.pass = other.pass = false;
      row.reason = other.reason = `Session ${session} is bound to both ${other.id} and ${row.id}`;
    } else owners.set(session, row);
  }
  entry.rows.sort((a, b) => `${a.cohort}-${a.provider}-${a.id}`.localeCompare(`${b.cohort}-${b.provider}-${b.id}`));
  entry.finished_at = new Date().toISOString();
  save();
}

async function main() {
  mkdirSync(out, { recursive: true });
  if (mode === "setup") await setup();
  else if (mode === "before") {
    await checkHealth();
    result.round_started_at = new Date().toISOString();
    result.log_baseline = logLines(join(dataDir, "logs")).map(digest);
    result.pre_restart_tabs = await tabs();
    save();
  } else if (mode === "round") await runRound(round);
  else if (mode === "abort") {
    result.fatal = `DEV restart ${round} failed; see restart-${round}.log`;
    captureLogs(round);
    save();
  } else if (mode === "finish") {
    const failed = result.coverage.some((entry) => entry.status === "required unavailable") ||
      result.rounds.some((entry) => entry.rows.some((row) => !row.pass && !row.observed_only)) ||
      result.rounds.filter((entry) => entry.round > 0).length !== 3;
    result.finished_at = new Date().toISOString();
    result.pass = !failed;
    save();
    writeFileSync(join(out, "complete"), failed ? "1\n" : "0\n");
    console.log(`Results: ${join(out, "results.md")}`);
  } else throw new Error(`Unknown mode ${mode}`);
}
main().catch((error) => {
  result.fatal = error.message;
  save();
  console.error(error.message);
  process.exitCode = 2;
});
